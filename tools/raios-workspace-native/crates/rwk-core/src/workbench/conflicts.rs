//! Git merge-conflict blocks in a file: where they are and how one is resolved.

/// One conflict block; every field is a zero-based line index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conflict {
    /// The `<<<<<<<` line.
    pub start: usize,
    /// The `|||||||` line of a diff3 block, when there is one.
    pub base: Option<usize>,
    /// The `=======` line.
    pub middle: usize,
    /// The `>>>>>>>` line.
    pub end: usize,
}

/// Which side of a conflict to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The current branch's lines (above `=======`).
    Ours,
    /// The incoming lines (below `=======`).
    Theirs,
    /// Both, ours first.
    Both,
}

fn is_marker(line: &str, mark: char) -> bool {
    let line = line.trim_end_matches(['\n', '\r']);
    let run = line.chars().take_while(|c| *c == mark).count();
    run == 7 && (line.len() == 7 || (mark != '=' && line[7..].starts_with(' ')))
}

/// Every complete conflict block of `text`, top to bottom. A block that is not closed is
/// ignored.
pub fn find(text: &str) -> Vec<Conflict> {
    let mut found = Vec::new();
    let mut open: Option<(usize, Option<usize>, Option<usize>)> = None;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        match &mut open {
            None if is_marker(line, '<') => open = Some((index, None, None)),
            Some((start, base, middle)) => {
                if is_marker(line, '<') {
                    // A new block starts before the old one closed: the old one was not real.
                    open = Some((index, None, None));
                } else if middle.is_none() && base.is_none() && is_marker(line, '|') {
                    *base = Some(index);
                } else if middle.is_none() && is_marker(line, '=') {
                    *middle = Some(index);
                } else if let (Some(middle), true) = (*middle, is_marker(line, '>')) {
                    found.push(Conflict {
                        start: *start,
                        base: *base,
                        middle,
                        end: index,
                    });
                    open = None;
                }
            }
            None => {}
        }
    }
    found
}

/// The conflict that contains the zero-based `line`.
pub fn at(text: &str, line: usize) -> Option<Conflict> {
    find(text)
        .into_iter()
        .find(|conflict| (conflict.start..=conflict.end).contains(&line))
}

/// The line of the first conflict that starts after `line`, wrapping to the first one.
pub fn next_after(text: &str, line: usize) -> Option<usize> {
    let all = find(text);
    all.iter()
        .find(|conflict| conflict.start > line)
        .or_else(|| all.first())
        .map(|conflict| conflict.start)
}

/// `text` with the conflict around `line` replaced by the chosen side. `None` when the line is
/// not inside a conflict.
pub fn resolve(text: &str, line: usize, side: Side) -> Option<String> {
    let conflict = at(text, line)?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let ours_end = conflict.base.unwrap_or(conflict.middle);
    let ours = &lines[conflict.start + 1..ours_end];
    let theirs = &lines[conflict.middle + 1..conflict.end];
    let mut kept: Vec<&str> = Vec::new();
    if matches!(side, Side::Ours | Side::Both) {
        kept.extend(ours);
    }
    if matches!(side, Side::Theirs | Side::Both) {
        // The last line of "ours" may lack a newline only at the end of a file; the markers
        // after it make that impossible, so the two parts always join on a line break.
        kept.extend(theirs);
    }
    let mut out: String = lines[..conflict.start].concat();
    out.push_str(&kept.concat());
    out.push_str(&lines[conflict.end + 1..].concat());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "head\n<<<<<<< HEAD\nours 1\nours 2\n=======\ntheirs\n>>>>>>> topic\ntail\n";

    #[test]
    fn a_block_is_found_with_its_marker_lines() {
        assert_eq!(
            find(TEXT),
            [Conflict {
                start: 1,
                base: None,
                middle: 4,
                end: 6
            }]
        );
        assert_eq!(at(TEXT, 3).map(|c| c.start), Some(1));
        assert_eq!(at(TEXT, 0), None);
        assert_eq!(at(TEXT, 7), None);
    }

    #[test]
    fn each_side_can_be_kept() {
        assert_eq!(
            resolve(TEXT, 2, Side::Ours).unwrap(),
            "head\nours 1\nours 2\ntail\n"
        );
        assert_eq!(
            resolve(TEXT, 5, Side::Theirs).unwrap(),
            "head\ntheirs\ntail\n"
        );
        assert_eq!(
            resolve(TEXT, 1, Side::Both).unwrap(),
            "head\nours 1\nours 2\ntheirs\ntail\n"
        );
        assert_eq!(resolve(TEXT, 0, Side::Ours), None);
    }

    #[test]
    fn diff3_blocks_drop_the_base_and_crlf_survives() {
        let text =
            "<<<<<<< a\r\nmine\r\n||||||| base\r\nold\r\n=======\r\nyours\r\n>>>>>>> b\r\nend\r\n";
        assert_eq!(find(text)[0].base, Some(2));
        assert_eq!(resolve(text, 0, Side::Ours).unwrap(), "mine\r\nend\r\n");
        assert_eq!(resolve(text, 0, Side::Theirs).unwrap(), "yours\r\nend\r\n");
    }

    #[test]
    fn only_complete_blocks_count_and_the_next_one_wraps() {
        let text = "<<<<<<< x\nno end\n\n<<<<<<< y\na\n=======\nb\n>>>>>>> z\nx = 1 ======= 2\n";
        assert_eq!(find(text).len(), 1);
        assert_eq!(find(text)[0].start, 3);
        assert_eq!(next_after(text, 0), Some(3));
        assert_eq!(next_after(text, 3), Some(3), "wraps to the only block");
        assert_eq!(next_after("plain\n", 0), None);
    }
}
