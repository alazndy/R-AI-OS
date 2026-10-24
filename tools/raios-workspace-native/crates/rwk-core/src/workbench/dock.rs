//! How the terminal dock is arranged: a tree of tab groups and splits.
//!
//! Pure data and pure operations, no toolkit: the UI draws the tree and turns gestures into
//! calls here, and the saved workspace stores it. The tree is generic over how a pane is
//! identified, because the same shape is used with live [`SessionId`](crate::sessions::SessionId)s
//! while the workspace runs and with positions in the saved terminal list on disk.
//!
//! A split has any number of children (at least two) with weights that sum to one, so twelve
//! terminals stacked in a column are one split of twelve, not eleven nested pairs, and the depth
//! limit [`MAX_DEPTH`] stays meaningful. Every operation either applies completely or changes
//! nothing: it works on a copy, normalizes it (empty groups vanish, a split with one child is
//! replaced by that child, a split inside a split of the same axis is merged into it) and checks
//! [`DockLayout::validate`] before the copy replaces the original.

use serde::{Deserialize, Serialize};

use crate::sessions::MAX_SESSIONS;

/// Deepest nesting of groups and splits, counting the root as one.
pub const MAX_DEPTH: usize = 4;

/// Smallest share of a split that a resize leaves to one child.
pub const MIN_SHARE: f32 = 0.05;

/// Direction in which the children of a split are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Side by side, in a row.
    Horizontal,
    /// One above the other, in a column.
    Vertical,
}

/// The edge of a pane that a dragged terminal is dropped on to split it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    /// The new pane goes to the left.
    Left,
    /// The new pane goes to the right.
    Right,
    /// The new pane goes above.
    Top,
    /// The new pane goes below.
    Bottom,
}

impl Edge {
    fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::Horizontal,
            Self::Top | Self::Bottom => Axis::Vertical,
        }
    }

    fn new_pane_first(self) -> bool {
        matches!(self, Self::Left | Self::Top)
    }
}

/// Where on a pane a dragged terminal was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    /// The middle: the terminal joins the pane's group as a tab.
    Center,
    /// Near an edge: the terminal is split off next to the pane on that edge.
    Edge(Edge),
}

/// How much of each side of a pane counts as its edge zone, as a fraction of the pane.
pub const EDGE_BAND: f32 = 0.25;

/// The zone under a pointer at `(fx, fy)`, fractions of the pane's width and height (values
/// outside 0 to 1 are clamped). In a corner the nearer edge wins; on a tie the order is left,
/// right, top, bottom.
pub fn zone_at(fx: f32, fy: f32) -> DropZone {
    let fx = if fx.is_finite() {
        fx.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let fy = if fy.is_finite() {
        fy.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let candidates = [
        (fx, Edge::Left),
        (1.0 - fx, Edge::Right),
        (fy, Edge::Top),
        (1.0 - fy, Edge::Bottom),
    ];
    let (distance, edge) = candidates
        .into_iter()
        .fold((f32::MAX, Edge::Left), |best, next| {
            if next.0 < best.0 {
                next
            } else {
                best
            }
        });
    if distance < EDGE_BAND {
        DropZone::Edge(edge)
    } else {
        DropZone::Center
    }
}

/// Why an operation was refused. The layout is unchanged when one is returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockError(pub &'static str);

impl std::fmt::Display for DockError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for DockError {}

/// One child of a split and its share of the split.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Child<T> {
    /// Share of the split, 0 to 1; the shares of a split sum to one.
    pub weight: f32,
    /// What the child shows.
    pub node: Node<T>,
}

/// A group of tabbed panes, or a split into several nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum Node<T> {
    /// Panes sharing one area; only the active one is shown.
    Tabs {
        /// The panes, in tab order.
        sessions: Vec<T>,
        /// Index of the shown pane.
        #[serde(default)]
        active: usize,
    },
    /// Children laid out along an axis.
    Split {
        /// The direction of the layout.
        axis: Axis,
        /// The children, in layout order.
        children: Vec<Child<T>>,
    },
}

impl<T> Node<T> {
    fn depth(&self) -> usize {
        match self {
            Self::Tabs { .. } => 1,
            Self::Split { children, .. } => {
                1 + children
                    .iter()
                    .map(|child| child.node.depth())
                    .max()
                    .unwrap_or(0)
            }
        }
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a T>) {
        match self {
            Self::Tabs { sessions, .. } => out.extend(sessions),
            Self::Split { children, .. } => {
                for child in children {
                    child.node.collect(out);
                }
            }
        }
    }

    fn single(id: T) -> Self {
        Self::Tabs {
            sessions: vec![id],
            active: 0,
        }
    }
}

/// Replaces unusable weights of a split by an equal share, drops empty groups, replaces a
/// split of one child by the child, merges a split into a parent split of the same axis, clamps
/// every active index and scales the weights of each split to sum to one.
fn normalize<T>(node: Node<T>) -> Option<Node<T>> {
    match node {
        Node::Tabs { sessions, active } => {
            if sessions.is_empty() {
                return None;
            }
            let active = active.min(sessions.len() - 1);
            Some(Node::Tabs { sessions, active })
        }
        Node::Split { axis, children } => {
            let mut flat: Vec<Child<T>> = Vec::new();
            for child in children {
                let weight = if child.weight.is_finite() && child.weight > 0.0 {
                    child.weight
                } else {
                    1.0
                };
                match normalize(child.node) {
                    None => {}
                    Some(Node::Split {
                        axis: inner_axis,
                        children: inner,
                    }) if inner_axis == axis => {
                        // `inner` is already normalized, so its weights sum to one.
                        for grandchild in inner {
                            flat.push(Child {
                                weight: weight * grandchild.weight,
                                node: grandchild.node,
                            });
                        }
                    }
                    Some(node) => flat.push(Child { weight, node }),
                }
            }
            match flat.len() {
                0 => None,
                1 => flat.pop().map(|child| child.node),
                _ => {
                    let total: f32 = flat.iter().map(|child| child.weight).sum();
                    for child in &mut flat {
                        child.weight /= total;
                    }
                    Some(Node::Split {
                        axis,
                        children: flat,
                    })
                }
            }
        }
    }
}

fn validate_node<T: PartialEq>(node: &Node<T>, parent_axis: Option<Axis>) -> Result<(), DockError> {
    match node {
        Node::Tabs { sessions, active } => {
            if sessions.is_empty() {
                return Err(DockError("a group has no panes"));
            }
            if *active >= sessions.len() {
                return Err(DockError("the active tab is out of range"));
            }
            Ok(())
        }
        Node::Split { axis, children } => {
            if children.len() < 2 {
                return Err(DockError("a split has fewer than two children"));
            }
            if parent_axis == Some(*axis) {
                return Err(DockError(
                    "a split sits directly inside a split of its axis",
                ));
            }
            let mut total = 0.0_f32;
            for child in children {
                if !(child.weight.is_finite() && child.weight > 0.0) {
                    return Err(DockError("a split weight is not a positive number"));
                }
                total += child.weight;
                validate_node(&child.node, Some(*axis))?;
            }
            if (total - 1.0).abs() > 1e-3 {
                return Err(DockError("the weights of a split do not sum to one"));
            }
            Ok(())
        }
    }
}

/// Child indices from the root to a node.
type Path = Vec<usize>;

fn path_of_group<T: PartialEq>(node: &Node<T>, id: &T) -> Option<Path> {
    match node {
        Node::Tabs { sessions, .. } => sessions.contains(id).then(Vec::new),
        Node::Split { children, .. } => children.iter().enumerate().find_map(|(index, child)| {
            path_of_group(&child.node, id).map(|mut path| {
                path.insert(0, index);
                path
            })
        }),
    }
}

fn node_at_mut<'a, T>(node: &'a mut Node<T>, path: &[usize]) -> Option<&'a mut Node<T>> {
    let Some((first, rest)) = path.split_first() else {
        return Some(node);
    };
    match node {
        Node::Split { children, .. } => node_at_mut(&mut children.get_mut(*first)?.node, rest),
        Node::Tabs { .. } => None,
    }
}

fn node_at<'a, T>(node: &'a Node<T>, path: &[usize]) -> Option<&'a Node<T>> {
    let Some((first, rest)) = path.split_first() else {
        return Some(node);
    };
    match node {
        Node::Split { children, .. } => node_at(&children.get(*first)?.node, rest),
        Node::Tabs { .. } => None,
    }
}

fn last_group_path<T>(node: &Node<T>) -> Path {
    match node {
        Node::Tabs { .. } => Vec::new(),
        Node::Split { children, .. } => {
            let last = children.len() - 1;
            let mut path = vec![last];
            path.extend(last_group_path(&children[last].node));
            path
        }
    }
}

/// The arrangement of the dock. `None` inside means no panes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DockLayout<T> {
    root: Option<Node<T>>,
}

impl<T> Default for DockLayout<T> {
    fn default() -> Self {
        Self { root: None }
    }
}

impl<T: Clone + PartialEq> DockLayout<T> {
    /// A layout with no panes.
    pub fn new() -> Self {
        Self::default()
    }

    /// The default arrangement: every pane in a group of its own, stacked in a column in the
    /// given order and sharing the height equally.
    pub fn stacked(ids: impl IntoIterator<Item = T>) -> Self {
        let mut layout = Self::new();
        for id in ids {
            // A duplicate or a pane beyond the ceiling is simply not added.
            let _ = layout.push_default(id);
        }
        layout
    }

    /// True when no pane is arranged.
    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// The tree, for drawing.
    pub fn root(&self) -> Option<&Node<T>> {
        self.root.as_ref()
    }

    /// Every pane, in layout order.
    pub fn sessions(&self) -> Vec<T> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.collect(&mut out);
        }
        out.into_iter().cloned().collect()
    }

    /// True when `id` is arranged.
    pub fn contains(&self, id: &T) -> bool {
        self.root
            .as_ref()
            .is_some_and(|root| path_of_group(root, id).is_some())
    }

    /// How many panes are arranged.
    pub fn len(&self) -> usize {
        self.sessions().len()
    }

    /// Checks every invariant: unique panes, at most [`MAX_SESSIONS`], depth at most
    /// [`MAX_DEPTH`], no empty group, splits of at least two children whose weights sum to one,
    /// no split directly inside a split of its axis, active tabs in range.
    pub fn validate(&self) -> Result<(), DockError> {
        let Some(root) = &self.root else {
            return Ok(());
        };
        validate_node(root, None)?;
        if root.depth() > MAX_DEPTH {
            return Err(DockError("the layout is nested too deeply"));
        }
        let sessions = self.sessions();
        if sessions.len() > MAX_SESSIONS {
            return Err(DockError("too many panes"));
        }
        for (index, id) in sessions.iter().enumerate() {
            if sessions[..index].contains(id) {
                return Err(DockError("a pane appears twice"));
            }
        }
        Ok(())
    }

    /// Applies `change` to a copy, normalizes and validates it, and only then replaces the
    /// layout.
    fn transact(
        &mut self,
        change: impl FnOnce(&mut Option<Node<T>>) -> Result<(), DockError>,
    ) -> Result<(), DockError> {
        let mut copy = self.root.clone();
        change(&mut copy)?;
        let next = Self {
            root: copy.and_then(normalize),
        };
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// Adds a pane as a group of its own at the end of the column. When the layout is too deep
    /// for that, the pane becomes a tab of the last group instead.
    pub fn push_default(&mut self, id: T) -> Result<(), DockError> {
        if self.contains(&id) {
            return Err(DockError("the pane is already arranged"));
        }
        if self.len() >= MAX_SESSIONS {
            return Err(DockError("too many panes"));
        }
        let as_group = {
            let id = id.clone();
            move |root: &mut Option<Node<T>>| {
                match root.take() {
                    None => *root = Some(Node::single(id)),
                    Some(Node::Split {
                        axis: Axis::Vertical,
                        mut children,
                    }) => {
                        // The newcomer weighs as much as the average child: equal shares stay equal.
                        let share = 1.0 / children.len() as f32;
                        children.push(Child {
                            weight: share,
                            node: Node::single(id),
                        });
                        *root = Some(Node::Split {
                            axis: Axis::Vertical,
                            children,
                        });
                    }
                    Some(other) => {
                        *root = Some(Node::Split {
                            axis: Axis::Vertical,
                            children: vec![
                                Child {
                                    weight: 1.0,
                                    node: other,
                                },
                                Child {
                                    weight: 1.0,
                                    node: Node::single(id),
                                },
                            ],
                        });
                    }
                }
                Ok(())
            }
        };
        if self.transact(as_group).is_ok() {
            return Ok(());
        }
        self.transact(|root| {
            let root = root.as_mut().ok_or(DockError("no group to join"))?;
            let path = last_group_path(root);
            match node_at_mut(root, &path) {
                Some(Node::Tabs { sessions, active }) => {
                    sessions.push(id);
                    *active = sessions.len() - 1;
                    Ok(())
                }
                _ => Err(DockError("no group to join")),
            }
        })
    }

    /// Removes a pane. `false` when it was not arranged. Empty groups and splits that lose a
    /// child collapse.
    pub fn remove(&mut self, id: &T) -> bool {
        if !self.contains(id) {
            return false;
        }
        self.transact(|root| {
            let root = root.as_mut().ok_or(DockError("empty"))?;
            take_out(root, id);
            Ok(())
        })
        .is_ok()
    }

    /// Keeps only the panes for which `keep` is true.
    pub fn retain(&mut self, keep: impl Fn(&T) -> bool) {
        for id in self.sessions() {
            if !keep(&id) {
                self.remove(&id);
            }
        }
    }

    /// Makes `id` the shown tab of its group.
    pub fn activate(&mut self, id: &T) -> bool {
        self.transact(|root| {
            let root = root.as_mut().ok_or(DockError("empty"))?;
            let path = path_of_group(root, id).ok_or(DockError("unknown pane"))?;
            if let Some(Node::Tabs { sessions, active }) = node_at_mut(root, &path) {
                *active = sessions.iter().position(|other| other == id).unwrap_or(0);
            }
            Ok(())
        })
        .is_ok()
    }

    /// Moves `id` into the group that `into` belongs to, at tab position `index` (clamped).
    /// When `into` is `id` itself, or in the same group, the tab is reordered.
    pub fn move_tab(&mut self, id: &T, into: &T, index: usize) -> Result<(), DockError> {
        self.transact(|root| {
            let root = root.as_mut().ok_or(DockError("empty"))?;
            if path_of_group(root, id).is_none() {
                return Err(DockError("unknown pane"));
            }
            // The target is located before the pane leaves its group; taking a pane out never
            // restructures the tree (that happens when the copy is normalized), so the path holds.
            let target = path_of_group(root, into).ok_or(DockError("unknown target"))?;
            take_out(root, id);
            match node_at_mut(root, &target) {
                Some(Node::Tabs { sessions, active }) => {
                    let at = index.min(sessions.len());
                    sessions.insert(at, id.clone());
                    *active = at;
                    Ok(())
                }
                _ => Err(DockError("unknown target")),
            }
        })
    }

    /// Takes `id` out of its group and puts it in a group of its own next to the group of
    /// `target`, on the given edge. Refused when `id` is the only pane of that group.
    pub fn split(&mut self, id: &T, target: &T, edge: Edge) -> Result<(), DockError> {
        self.transact(|root| {
            let root = root.as_mut().ok_or(DockError("empty"))?;
            let source = path_of_group(root, id).ok_or(DockError("unknown pane"))?;
            let target_path = path_of_group(root, target).ok_or(DockError("unknown target"))?;
            if source == target_path {
                let alone = matches!(
                    node_at_mut(root, &source),
                    Some(Node::Tabs { sessions, .. }) if sessions.len() == 1
                );
                if alone {
                    return Err(DockError("a lone pane cannot be split from itself"));
                }
            }
            take_out(root, id);
            let node = node_at_mut(root, &target_path).ok_or(DockError("unknown target"))?;
            let existing = std::mem::replace(
                node,
                Node::Tabs {
                    sessions: Vec::new(),
                    active: 0,
                },
            );
            let added = Child {
                weight: 1.0,
                node: Node::single(id.clone()),
            };
            let kept = Child {
                weight: 1.0,
                node: existing,
            };
            *node = Node::Split {
                axis: edge.axis(),
                children: if edge.new_pane_first() {
                    vec![added, kept]
                } else {
                    vec![kept, added]
                },
            };
            Ok(())
        })
    }

    /// Moves the boundary after child `boundary` of the split at `path` by `delta`, a fraction
    /// of the whole split (positive grows the first child). Neither neighbour goes below
    /// [`MIN_SHARE`].
    pub fn resize(&mut self, path: &[usize], boundary: usize, delta: f32) -> Result<(), DockError> {
        if !delta.is_finite() {
            return Err(DockError("the resize amount is not a number"));
        }
        self.transact(|root| {
            let root = root.as_mut().ok_or(DockError("empty"))?;
            let Some(Node::Split { children, .. }) = node_at_mut(root, path) else {
                return Err(DockError("no split at that path"));
            };
            if boundary + 1 >= children.len() {
                return Err(DockError("no such boundary"));
            }
            let pair = children[boundary].weight + children[boundary + 1].weight;
            if pair < 2.0 * MIN_SHARE {
                return Err(DockError("the neighbours are already at their minimum"));
            }
            let first = (children[boundary].weight + delta).clamp(MIN_SHARE, pair - MIN_SHARE);
            children[boundary].weight = first;
            children[boundary + 1].weight = pair - first;
            Ok(())
        })
    }

    /// The pane `delta` places away from `id` in layout order, without wrapping.
    pub fn neighbor(&self, id: &T, delta: isize) -> Option<T> {
        let sessions = self.sessions();
        let at = sessions.iter().position(|other| other == id)? as isize;
        let target = usize::try_from(at + delta).ok()?;
        sessions.get(target).cloned()
    }

    /// Another pane of the group `id` is in, when the group has several.
    pub fn group_mate(&self, id: &T) -> Option<T> {
        let root = self.root.as_ref()?;
        let path = path_of_group(root, id)?;
        let mut copy = root.clone();
        match node_at_mut(&mut copy, &path)? {
            Node::Tabs { sessions, .. } => sessions.iter().find(|other| *other != id).cloned(),
            Node::Split { .. } => None,
        }
    }

    /// True when `id` is the shown tab of its group.
    pub fn is_shown(&self, id: &T) -> bool {
        let Some(root) = self.root.as_ref() else {
            return false;
        };
        let Some(path) = path_of_group(root, id) else {
            return false;
        };
        matches!(
            node_at(root, &path),
            Some(Node::Tabs { sessions, active }) if sessions.get(*active) == Some(id)
        )
    }

    /// The position of `id` among the tabs of its group.
    pub fn tab_position(&self, id: &T) -> Option<usize> {
        let root = self.root.as_ref()?;
        let path = path_of_group(root, id)?;
        match node_at(root, &path)? {
            Node::Tabs { sessions, .. } => sessions.iter().position(|other| other == id),
            Node::Split { .. } => None,
        }
    }

    /// Exchanges the places of two panes; the shape of the layout does not change.
    pub fn swap(&mut self, first: &T, second: &T) -> Result<(), DockError> {
        if first == second || !self.contains(first) || !self.contains(second) {
            return Err(DockError("two different arranged panes are needed"));
        }
        fn go<T: Clone + PartialEq>(node: &mut Node<T>, first: &T, second: &T) {
            match node {
                Node::Tabs { sessions, .. } => {
                    for id in sessions {
                        if id == first {
                            *id = second.clone();
                        } else if id == second {
                            *id = first.clone();
                        }
                    }
                }
                Node::Split { children, .. } => {
                    for child in children {
                        go(&mut child.node, first, second);
                    }
                }
            }
        }
        self.transact(|root| {
            go(root.as_mut().ok_or(DockError("empty"))?, first, second);
            Ok(())
        })
    }

    /// Takes `id` out of a group of several tabs into a group of its own below that group.
    pub fn ungroup(&mut self, id: &T) -> Result<(), DockError> {
        let mate = self
            .group_mate(id)
            .ok_or(DockError("the pane is not in a group of several"))?;
        self.split(id, &mate, Edge::Bottom)
    }

    /// The nearest split around `id` and which of its children holds the pane's group.
    pub fn enclosing_split(&self, id: &T) -> Option<(Vec<usize>, usize)> {
        let mut path = path_of_group(self.root.as_ref()?, id)?;
        let child = path.pop()?;
        Some((path, child))
    }

    /// Grows the share of `id`'s group by `delta` (a fraction of the enclosing split) at the
    /// cost of its neighbour; a negative `delta` shrinks it. Refused when nothing encloses it.
    pub fn grow(&mut self, id: &T, delta: f32) -> Result<(), DockError> {
        let (path, child) = self
            .enclosing_split(id)
            .ok_or(DockError("the pane fills the dock"))?;
        let count = match self.root.as_ref().and_then(|root| node_at(root, &path)) {
            Some(Node::Split { children, .. }) => children.len(),
            _ => return Err(DockError("no split at that path")),
        };
        if child + 1 < count {
            self.resize(&path, child, delta)
        } else {
            self.resize(&path, child - 1, -delta)
        }
    }

    /// Puts the boundary after child `boundary` of the split at `path` at `position`, a
    /// fraction (0 to 1) of the whole split measured from its start. This is what dragging a
    /// divider does: the pointer gives a position, not a distance.
    pub fn set_boundary(
        &mut self,
        path: &[usize],
        boundary: usize,
        position: f32,
    ) -> Result<(), DockError> {
        let current = match self.root.as_ref().and_then(|root| node_at(root, path)) {
            Some(Node::Split { children, .. }) if boundary + 1 < children.len() => children
                [..=boundary]
                .iter()
                .map(|child| child.weight)
                .sum::<f32>(),
            _ => return Err(DockError("no such boundary")),
        };
        self.resize(path, boundary, position - current)
    }

    /// The same arrangement over other pane identifiers.
    pub fn map<U>(&self, convert: &impl Fn(&T) -> U) -> DockLayout<U> {
        fn go<T, U>(node: &Node<T>, convert: &impl Fn(&T) -> U) -> Node<U> {
            match node {
                Node::Tabs { sessions, active } => Node::Tabs {
                    sessions: sessions.iter().map(convert).collect(),
                    active: *active,
                },
                Node::Split { axis, children } => Node::Split {
                    axis: *axis,
                    children: children
                        .iter()
                        .map(|child| Child {
                            weight: child.weight,
                            node: go(&child.node, convert),
                        })
                        .collect(),
                },
            }
        }
        DockLayout {
            root: self.root.as_ref().map(|root| go(root, convert)),
        }
    }

    /// The same arrangement over other identifiers, dropping the panes `convert` has no
    /// identifier for (the layout around them closes up).
    pub fn filter_map<U: Clone + PartialEq>(
        &self,
        convert: &impl Fn(&T) -> Option<U>,
    ) -> DockLayout<U> {
        fn go<T, U>(node: &Node<T>, convert: &impl Fn(&T) -> Option<U>) -> Node<U> {
            match node {
                Node::Tabs { sessions, active } => Node::Tabs {
                    sessions: sessions.iter().filter_map(convert).collect(),
                    active: *active,
                },
                Node::Split { axis, children } => Node::Split {
                    axis: *axis,
                    children: children
                        .iter()
                        .map(|child| Child {
                            weight: child.weight,
                            node: go(&child.node, convert),
                        })
                        .collect(),
                },
            }
        }
        DockLayout {
            root: self
                .root
                .as_ref()
                .and_then(|root| normalize(go(root, convert))),
        }
        .sanitized()
        .unwrap_or_default()
    }

    /// Makes untrusted state (a layout read from a file) safe to use: repeated panes keep their
    /// first place, panes beyond the ceiling are dropped, and the tree is normalized. `None`
    /// when what is left is nested too deeply, so the caller falls back to the default
    /// arrangement instead of guessing.
    pub fn sanitized(self) -> Option<Self> {
        let mut seen: Vec<T> = Vec::new();
        let root = self
            .root
            .map(|root| dedupe(root, &mut seen))
            .and_then(normalize);
        let layout = Self { root };
        layout.validate().ok()?;
        Some(layout)
    }
}

/// Keeps the first occurrence of every pane, in layout order, and none beyond the ceiling.
fn dedupe<T: Clone + PartialEq>(node: Node<T>, seen: &mut Vec<T>) -> Node<T> {
    match node {
        Node::Tabs {
            mut sessions,
            active,
        } => {
            sessions.retain(|id| {
                if seen.contains(id) || seen.len() >= MAX_SESSIONS {
                    false
                } else {
                    seen.push(id.clone());
                    true
                }
            });
            Node::Tabs { sessions, active }
        }
        Node::Split { axis, children } => Node::Split {
            axis,
            children: children
                .into_iter()
                .map(|child| Child {
                    weight: child.weight,
                    node: dedupe(child.node, seen),
                })
                .collect(),
        },
    }
}

/// Removes the first occurrence of `id` found, depth first.
fn take_out<T: PartialEq>(node: &mut Node<T>, id: &T) -> bool {
    match node {
        Node::Tabs { sessions, active } => {
            if let Some(position) = sessions.iter().position(|other| other == id) {
                sessions.remove(position);
                if *active >= sessions.len() {
                    *active = sessions.len().saturating_sub(1);
                } else if position < *active {
                    *active -= 1;
                }
                true
            } else {
                false
            }
        }
        Node::Split { children, .. } => children
            .iter_mut()
            .any(|child| take_out(&mut child.node, id)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(ids: &[u32]) -> DockLayout<u32> {
        DockLayout::stacked(ids.iter().copied())
    }

    fn weights(layout: &DockLayout<u32>) -> Vec<f32> {
        match layout.root() {
            Some(Node::Split { children, .. }) => children.iter().map(|c| c.weight).collect(),
            _ => Vec::new(),
        }
    }

    fn assert_close(left: &[f32], right: &[f32]) {
        assert_eq!(left.len(), right.len(), "{left:?} vs {right:?}");
        for (a, b) in left.iter().zip(right) {
            assert!((a - b).abs() < 1e-4, "{left:?} vs {right:?}");
        }
    }

    #[test]
    fn a_stack_shares_the_height_equally_and_keeps_the_order() {
        let stack = layout(&[1, 2, 3, 4]);
        assert_eq!(stack.sessions(), [1, 2, 3, 4]);
        assert_close(&weights(&stack), &[0.25; 4]);
        assert_eq!(stack.validate(), Ok(()));

        let one = layout(&[7]);
        assert!(matches!(one.root(), Some(Node::Tabs { .. })));
        assert!(layout(&[]).is_empty());
    }

    #[test]
    fn twelve_panes_stack_flat_and_a_thirteenth_is_refused() {
        let ids: Vec<u32> = (0..12).collect();
        let mut stack = layout(&ids);
        assert_eq!(stack.len(), MAX_SESSIONS);
        assert_eq!(stack.root().map(Node::depth), Some(2));
        assert!(stack.push_default(99).is_err());
        assert_eq!(stack.len(), MAX_SESSIONS, "refusal changes nothing");
    }

    #[test]
    fn a_pane_cannot_be_added_twice() {
        let mut stack = layout(&[1, 2]);
        assert!(stack.push_default(2).is_err());
        assert_eq!(stack.sessions(), [1, 2]);
    }

    #[test]
    fn removing_panes_collapses_the_layout_around_them() {
        let mut stack = layout(&[1, 2, 3]);
        assert!(stack.remove(&2));
        assert_close(&weights(&stack), &[0.5, 0.5]);
        assert!(stack.remove(&1));
        assert!(
            matches!(stack.root(), Some(Node::Tabs { .. })),
            "one child left"
        );
        assert!(stack.remove(&3));
        assert!(stack.is_empty());
        assert!(!stack.remove(&3), "already gone");
    }

    #[test]
    fn dropping_on_a_side_edge_makes_a_row_inside_the_column() {
        let mut stack = layout(&[1, 2, 3]);
        stack.split(&3, &1, Edge::Right).unwrap();
        assert_eq!(stack.sessions(), [1, 3, 2]);
        let Some(Node::Split { axis, children }) = stack.root() else {
            panic!("a column");
        };
        assert_eq!(*axis, Axis::Vertical);
        assert_eq!(children.len(), 2);
        assert!(matches!(
            &children[0].node,
            Node::Split { axis: Axis::Horizontal, children } if children.len() == 2
        ));
        assert_eq!(stack.validate(), Ok(()));
    }

    #[test]
    fn dropping_on_a_matching_edge_joins_the_existing_split_instead_of_nesting() {
        let mut stack = layout(&[1, 2, 3]);
        stack.split(&3, &2, Edge::Bottom).unwrap();
        // The new pane sits below 2, in the same column; no column inside the column.
        assert_eq!(stack.sessions(), [1, 2, 3]);
        assert_eq!(stack.root().map(Node::depth), Some(2));
        // Pane 3's old third is shared out again, then 2 and 3 split their column's share.
        assert_close(&weights(&stack), &[0.5, 0.25, 0.25]);

        stack.split(&1, &3, Edge::Top).unwrap();
        assert_eq!(stack.sessions(), [2, 1, 3]);
        assert_eq!(stack.validate(), Ok(()));
    }

    #[test]
    fn a_lone_pane_cannot_be_split_from_itself_and_nothing_changes() {
        let mut stack = layout(&[1, 2]);
        let before = stack.clone();
        assert!(stack.split(&1, &1, Edge::Right).is_err());
        assert!(stack.split(&1, &9, Edge::Right).is_err());
        assert!(stack.split(&9, &1, Edge::Right).is_err());
        assert_eq!(stack, before);
    }

    #[test]
    fn tabs_move_between_groups_and_reorder_inside_one() {
        let mut stack = layout(&[1, 2, 3]);
        stack.move_tab(&3, &1, 1).unwrap();
        assert_eq!(stack.sessions(), [1, 3, 2]);
        assert!(matches!(
            stack.root(),
            Some(Node::Split { children, .. }) if children.len() == 2
        ));
        // The moved tab is the shown one.
        let Some(Node::Split { children, .. }) = stack.root() else {
            panic!("a column");
        };
        assert!(matches!(
            &children[0].node,
            Node::Tabs { sessions, active: 1 } if sessions == &[1, 3]
        ));

        stack.move_tab(&3, &3, 0).unwrap();
        assert_eq!(stack.sessions(), [3, 1, 2]);
        stack.move_tab(&3, &1, 99).unwrap();
        assert_eq!(stack.sessions(), [1, 3, 2], "the index is clamped");

        assert!(stack.move_tab(&9, &1, 0).is_err());
        assert!(stack.move_tab(&1, &9, 0).is_err());
        assert_eq!(stack.sessions(), [1, 3, 2]);
    }

    #[test]
    fn moving_the_only_pane_of_a_group_closes_that_group() {
        let mut stack = layout(&[1, 2, 3]);
        stack.move_tab(&2, &3, 0).unwrap();
        assert_eq!(stack.sessions(), [1, 2, 3]);
        assert_close(&weights(&stack), &[0.5, 0.5]);
    }

    #[test]
    fn activating_a_tab_shows_it() {
        let mut stack = layout(&[1, 2]);
        stack.move_tab(&2, &1, 1).unwrap();
        assert!(stack.activate(&1));
        assert!(matches!(
            stack.root(),
            Some(Node::Tabs { active: 0, sessions }) if sessions == &[1, 2]
        ));
        assert!(!stack.activate(&9));
    }

    #[test]
    fn nesting_beyond_the_depth_limit_is_refused_and_leaves_the_layout_alone() {
        let ids: Vec<u32> = (0..8).collect();
        let mut stack = layout(&ids);
        let mut refused = false;
        for index in 1..8_u32 {
            let before = stack.clone();
            let edge = if index % 2 == 0 {
                Edge::Right
            } else {
                Edge::Bottom
            };
            // Always drop on the newest pane, which nests one level deeper each time.
            match stack.split(&index, &(index - 1), edge) {
                Ok(()) => {}
                Err(error) => {
                    refused = true;
                    assert_eq!(error, DockError("the layout is nested too deeply"));
                    assert_eq!(stack, before);
                }
            }
            assert_eq!(stack.validate(), Ok(()));
            assert!(stack.root().map_or(0, Node::depth) <= MAX_DEPTH);
        }
        assert!(refused, "the limit must be reachable");
    }

    #[test]
    fn a_new_pane_joins_the_last_group_as_a_tab_when_the_tree_is_too_deep() {
        // A row of [column of [row of [1, 2], 3], 4]: full depth, and a row at the root, so a
        // new column group would nest one level too far.
        let text = r#"{"node":"split","axis":"horizontal","children":[
            {"weight":0.5,"node":{"node":"split","axis":"vertical","children":[
                {"weight":0.5,"node":{"node":"split","axis":"horizontal","children":[
                    {"weight":0.5,"node":{"node":"tabs","sessions":[1]}},
                    {"weight":0.5,"node":{"node":"tabs","sessions":[2]}}]}},
                {"weight":0.5,"node":{"node":"tabs","sessions":[3]}}]}},
            {"weight":0.5,"node":{"node":"tabs","sessions":[4]}}]}"#;
        let mut stack: DockLayout<u32> = serde_json::from_str::<DockLayout<u32>>(text)
            .unwrap()
            .sanitized()
            .expect("valid at the limit");
        assert_eq!(stack.root().map(Node::depth), Some(MAX_DEPTH));

        stack.push_default(100).unwrap();
        assert_eq!(stack.sessions(), [1, 2, 3, 4, 100]);
        assert_eq!(stack.root().map(Node::depth), Some(MAX_DEPTH));
        let Some(Node::Split { children, .. }) = stack.root() else {
            panic!("a row");
        };
        assert!(matches!(
            &children[1].node,
            Node::Tabs { sessions, active: 1 } if sessions == &[4, 100]
        ));
    }

    #[test]
    fn resizing_moves_one_boundary_and_never_starves_a_neighbour() {
        let mut stack = layout(&[1, 2, 3]);
        stack.resize(&[], 0, 0.1).unwrap();
        let w = weights(&stack);
        assert!((w[0] - (1.0 / 3.0 + 0.1)).abs() < 1e-4);
        assert!((w[1] - (1.0 / 3.0 - 0.1)).abs() < 1e-4);
        assert!(
            (w[2] - 1.0 / 3.0).abs() < 1e-4,
            "the far neighbour is untouched"
        );

        stack.resize(&[], 0, 5.0).unwrap();
        let w = weights(&stack);
        assert!((w[1] - MIN_SHARE).abs() < 1e-4, "{w:?}");
        stack.resize(&[], 0, -5.0).unwrap();
        let w = weights(&stack);
        assert!((w[0] - MIN_SHARE).abs() < 1e-4, "{w:?}");

        assert!(stack.resize(&[], 2, 0.1).is_err());
        assert!(
            stack.resize(&[0], 0, 0.1).is_err(),
            "a group is not a split"
        );
        assert!(stack.resize(&[], 0, f32::NAN).is_err());
        assert_eq!(stack.validate(), Ok(()));
    }

    #[test]
    fn untrusted_layouts_are_cleaned_or_refused() {
        // A repeated pane keeps its first place; a bad weight becomes an equal share.
        let text = r#"{"node":"split","axis":"vertical","children":[
            {"weight":-3.0,"node":{"node":"tabs","sessions":[1,2],"active":9}},
            {"weight":2.0,"node":{"node":"tabs","sessions":[2,3]}}
        ]}"#;
        let parsed: DockLayout<u32> = serde_json::from_str(text).unwrap();
        let clean = parsed.sanitized().expect("repairable");
        assert_eq!(clean.sessions(), [1, 2, 3]);
        assert_eq!(clean.validate(), Ok(()));

        // Nothing but nesting is not repairable.
        fn deep(level: usize) -> Node<u32> {
            if level == 0 {
                return Node::single(level as u32);
            }
            Node::Split {
                axis: if level.is_multiple_of(2) {
                    Axis::Horizontal
                } else {
                    Axis::Vertical
                },
                children: vec![
                    Child {
                        weight: 0.5,
                        node: Node::single(level as u32 + 100),
                    },
                    Child {
                        weight: 0.5,
                        node: deep(level - 1),
                    },
                ],
            }
        }
        let too_deep = DockLayout {
            root: Some(deep(8)),
        };
        assert!(too_deep.sanitized().is_none());

        // More panes than the ceiling are dropped from the end.
        let many = DockLayout {
            root: Some(Node::Tabs {
                sessions: (0..30).collect::<Vec<u32>>(),
                active: 0,
            }),
        };
        assert_eq!(many.sanitized().expect("trimmed").len(), MAX_SESSIONS);
    }

    #[test]
    fn the_saved_shape_round_trips_and_old_files_without_a_layout_still_parse() {
        let mut stack = layout(&[0, 1, 2]);
        stack.split(&2, &0, Edge::Right).unwrap();
        let text = serde_json::to_string(&stack).unwrap();
        let back: DockLayout<u32> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, stack);

        let empty: DockLayout<u32> = serde_json::from_str("null").unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn mapping_identifiers_keeps_the_shape_and_dropping_some_closes_up() {
        let mut stack = layout(&[0, 1, 2, 3]);
        stack.split(&3, &0, Edge::Right).unwrap();
        let named = stack.map(&|id: &u32| format!("s{id}"));
        assert_eq!(named.sessions(), ["s0", "s3", "s1", "s2"]);

        let kept = stack.filter_map(&|id: &u32| id.is_multiple_of(2).then_some(*id * 10));
        assert_eq!(kept.sessions(), [0, 20]);
        assert_eq!(kept.validate(), Ok(()));
        assert!(stack.filter_map(&|_: &u32| None::<u32>).is_empty());
    }

    /// A tiny deterministic generator: the test must fail the same way every time.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self, below: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 33) as usize) % below.max(1)
        }
    }

    #[test]
    fn any_sequence_of_operations_keeps_every_invariant_and_loses_no_pane() {
        for seed in 0..40_u64 {
            let mut random = Lcg(seed + 1);
            let mut stack: DockLayout<u32> = DockLayout::new();
            let mut present: Vec<u32> = Vec::new();
            let mut next_id = 0_u32;
            for step in 0..300 {
                let before = stack.clone();
                let pick = |random: &mut Lcg, present: &[u32]| -> u32 {
                    if present.is_empty() || random.next(8) == 0 {
                        9_999
                    } else {
                        present[random.next(present.len())]
                    }
                };
                let result = match random.next(7) {
                    0 | 1 => {
                        let id = next_id;
                        next_id += 1;
                        let result = stack.push_default(id);
                        if result.is_ok() {
                            present.push(id);
                        }
                        result
                    }
                    2 => {
                        let id = pick(&mut random, &present);
                        if stack.remove(&id) {
                            present.retain(|other| *other != id);
                        }
                        Ok(())
                    }
                    3 => {
                        let id = pick(&mut random, &present);
                        let target = pick(&mut random, &present);
                        let edge =
                            [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom][random.next(4)];
                        stack.split(&id, &target, edge)
                    }
                    4 => {
                        let id = pick(&mut random, &present);
                        let target = pick(&mut random, &present);
                        let index = random.next(5);
                        stack.move_tab(&id, &target, index)
                    }
                    5 => {
                        let id = pick(&mut random, &present);
                        stack.activate(&id);
                        Ok(())
                    }
                    _ => {
                        let delta = (random.next(200) as f32 - 100.0) / 100.0;
                        let boundary = random.next(4);
                        stack.resize(&[], boundary, delta)
                    }
                };
                if result.is_err() {
                    assert_eq!(
                        stack, before,
                        "seed {seed} step {step}: a refusal must change nothing"
                    );
                }
                assert_eq!(stack.validate(), Ok(()), "seed {seed} step {step}");
                let mut have = stack.sessions();
                have.sort_unstable();
                let mut want = present.clone();
                want.sort_unstable();
                assert_eq!(
                    have, want,
                    "seed {seed} step {step}: panes were lost or invented"
                );
            }
        }
    }

    #[test]
    fn a_pointer_near_an_edge_splits_and_the_middle_joins() {
        assert_eq!(zone_at(0.5, 0.5), DropZone::Center);
        assert_eq!(zone_at(0.1, 0.5), DropZone::Edge(Edge::Left));
        assert_eq!(zone_at(0.9, 0.5), DropZone::Edge(Edge::Right));
        assert_eq!(zone_at(0.5, 0.05), DropZone::Edge(Edge::Top));
        assert_eq!(zone_at(0.5, 0.95), DropZone::Edge(Edge::Bottom));
        // A corner goes to the nearer edge; the band itself is not part of the middle.
        assert_eq!(zone_at(0.2, 0.1), DropZone::Edge(Edge::Top));
        assert_eq!(zone_at(0.1, 0.2), DropZone::Edge(Edge::Left));
        assert_eq!(zone_at(EDGE_BAND, 0.5), DropZone::Center);
        // Nonsense stays inside the pane.
        assert_eq!(zone_at(-3.0, 0.5), DropZone::Edge(Edge::Left));
        assert_eq!(zone_at(f32::NAN, f32::NAN), DropZone::Center);
    }

    #[test]
    fn neighbours_follow_layout_order_without_wrapping() {
        let stack = layout(&[1, 2, 3]);
        assert_eq!(stack.neighbor(&2, -1), Some(1));
        assert_eq!(stack.neighbor(&2, 1), Some(3));
        assert_eq!(stack.neighbor(&1, -1), None);
        assert_eq!(stack.neighbor(&3, 1), None);
        assert_eq!(stack.neighbor(&9, 1), None);
    }

    #[test]
    fn swapping_exchanges_places_and_keeps_the_shape() {
        let mut stack = layout(&[1, 2, 3]);
        stack.split(&3, &1, Edge::Right).unwrap();
        let shape = stack.map(&|_: &u32| 0_u32);
        stack.swap(&1, &2).unwrap();
        assert_eq!(stack.sessions(), [2, 3, 1]);
        assert_eq!(stack.map(&|_: &u32| 0_u32), shape);
        assert!(stack.swap(&1, &1).is_err());
        assert!(stack.swap(&1, &9).is_err());
    }

    #[test]
    fn ungrouping_puts_a_tab_below_its_old_group() {
        let mut stack = layout(&[1, 2]);
        stack.move_tab(&2, &1, 1).unwrap();
        assert_eq!(stack.group_mate(&2), Some(1));
        stack.ungroup(&2).unwrap();
        assert_eq!(stack.sessions(), [1, 2]);
        assert_eq!(stack.group_mate(&2), None);
        assert!(
            stack.ungroup(&2).is_err(),
            "no longer in a group of several"
        );
    }

    #[test]
    fn growing_a_pane_takes_from_its_neighbour_and_the_last_pane_takes_from_the_one_before() {
        let mut stack = layout(&[1, 2, 3]);
        stack.grow(&1, 0.1).unwrap();
        let w = weights(&stack);
        assert!((w[0] - (1.0 / 3.0 + 0.1)).abs() < 1e-4 && (w[1] - (1.0 / 3.0 - 0.1)).abs() < 1e-4);
        stack.grow(&3, 0.1).unwrap();
        let w = weights(&stack);
        assert!((w[2] - (1.0 / 3.0 + 0.1)).abs() < 1e-4, "{w:?}");
        assert!((w[1] - (1.0 / 3.0 - 0.2)).abs() < 1e-4, "{w:?}");
        assert!(
            layout(&[1]).grow(&1, 0.1).is_err(),
            "a lone pane fills the dock"
        );
        assert_eq!(stack.validate(), Ok(()));
    }

    #[test]
    fn dragging_a_divider_sets_the_boundary_to_the_pointer_position() {
        let mut stack = layout(&[1, 2, 3, 4]);
        stack.set_boundary(&[], 1, 0.7).unwrap();
        let w = weights(&stack);
        assert!(((w[0] + w[1]) - 0.7).abs() < 1e-4, "{w:?}");
        // The neighbour after the boundary gives up the difference; the others are untouched.
        assert!(
            (w[2] - 0.05).abs() < 1e-4 && (w[3] - 0.25).abs() < 1e-4,
            "{w:?}"
        );
        // Past the neighbour's minimum the boundary stops there.
        stack.set_boundary(&[], 2, 2.0).unwrap();
        let w = weights(&stack);
        assert!((w[3] - MIN_SHARE).abs() < 1e-4, "{w:?}");
        assert!(stack.set_boundary(&[], 3, 0.5).is_err());
        assert!(stack.set_boundary(&[], 0, f32::NAN).is_err());
    }
}
