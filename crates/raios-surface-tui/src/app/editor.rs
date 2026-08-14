use crossterm::event::{KeyCode, KeyEvent};

/// Generates a simple line-by-line diff for display.
pub fn simple_diff(old: &str, new: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let max = old_lines.len().max(new_lines.len());
    for i in 0..max {
        let o = old_lines.get(i);
        let n = new_lines.get(i);

        match (o, n) {
            (Some(o_val), Some(n_val)) => {
                if o_val == n_val {
                    lines.push(format!("  {}", o_val));
                } else {
                    lines.push(format!("- {}", o_val));
                    lines.push(format!("+ {}", n_val));
                }
            }
            (Some(o_val), None) => {
                lines.push(format!("- {}", o_val));
            }
            (None, Some(n_val)) => {
                lines.push(format!("+ {}", n_val));
            }
            (None, None) => {}
        }
    }
    lines
}

// ─── Simple line editor ───────────────────────────────────────────────────────

/// In-memory multiline text editor state for TUI forms and constitution editing.
#[derive(Debug, Default)]
pub struct Editor {
    /// Buffer lines of text.
    pub lines: Vec<String>,
    /// 0-indexed cursor row position.
    pub cursor_row: usize,
    /// 0-indexed cursor column position (in characters).
    pub cursor_col: usize,
    /// 0-indexed top row displayed in the visible window.
    pub scroll: usize,
    /// Height of the visible editor view area in rows.
    pub view_height: usize,
}

impl Editor {
    /// Constructs a new `Editor` initialized with text content lines and a view height limit.
    pub fn from_content(content: &str, view_height: usize) -> Self {
        let lines: Vec<String> = content.lines().map(str::to_owned).collect();
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll: 0,
            view_height,
        }
    }

    /// Joins all line buffer entries into a single newline-delimited text string.
    pub fn content(&self) -> String {
        self.lines.join("\n")
    }

    /// Processes a keyboard navigation or editing key event, updating cursor and scroll positions.
    pub fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                let byte = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                self.lines[self.cursor_row].insert(byte, c);
                self.cursor_col += 1;
            }
            KeyCode::Enter => {
                let byte = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                let rest = self.lines[self.cursor_row].split_off(byte);
                self.cursor_row += 1;
                self.lines.insert(self.cursor_row, rest);
                self.cursor_col = 0;
            }
            KeyCode::Backspace => {
                if self.cursor_col > 0 {
                    let b_end = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                    let b_start = char_to_byte(&self.lines[self.cursor_row], self.cursor_col - 1);
                    self.lines[self.cursor_row].drain(b_start..b_end);
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    let line = self.lines.remove(self.cursor_row);
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].chars().count();
                    self.lines[self.cursor_row].push_str(&line);
                }
            }
            KeyCode::Delete => {
                let line_len = self.lines[self.cursor_row].chars().count();
                if self.cursor_col < line_len {
                    let b_start = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
                    let b_end = char_to_byte(&self.lines[self.cursor_row], self.cursor_col + 1);
                    self.lines[self.cursor_row].drain(b_start..b_end);
                } else if self.cursor_row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.cursor_row + 1);
                    self.lines[self.cursor_row].push_str(&next);
                }
            }
            KeyCode::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].chars().count();
                }
            }
            KeyCode::Right => {
                let line_len = self.lines[self.cursor_row].chars().count();
                if self.cursor_col < line_len {
                    self.cursor_col += 1;
                } else if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_col = 0;
                }
            }
            KeyCode::Up if self.cursor_row > 0 => {
                self.cursor_row -= 1;
                let max = self.lines[self.cursor_row].chars().count();
                self.cursor_col = self.cursor_col.min(max);
            }
            KeyCode::Down if self.cursor_row + 1 < self.lines.len() => {
                self.cursor_row += 1;
                let max = self.lines[self.cursor_row].chars().count();
                self.cursor_col = self.cursor_col.min(max);
            }
            KeyCode::Home => self.cursor_col = 0,
            KeyCode::End => self.cursor_col = self.lines[self.cursor_row].chars().count(),
            KeyCode::PageUp => {
                self.cursor_row = self.cursor_row.saturating_sub(self.view_height);
                self.cursor_col = self
                    .cursor_col
                    .min(self.lines[self.cursor_row].chars().count());
            }
            KeyCode::PageDown => {
                self.cursor_row = (self.cursor_row + self.view_height).min(self.lines.len() - 1);
                self.cursor_col = self
                    .cursor_col
                    .min(self.lines[self.cursor_row].chars().count());
            }
            _ => {}
        }
        self.update_scroll();
    }

    fn update_scroll(&mut self) {
        if self.view_height == 0 {
            return;
        }
        if self.cursor_row < self.scroll {
            self.scroll = self.cursor_row;
        } else if self.cursor_row >= self.scroll + self.view_height {
            self.scroll = self.cursor_row + 1 - self.view_height;
        }
    }
}

/// Converts a 0-based character column position into a byte offset within a UTF-8 string.
pub fn char_to_byte(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    // ─── char_to_byte ───────────────────────────────────────────────────────

    #[test]
    fn char_to_byte_handles_ascii() {
        assert_eq!(char_to_byte("hello", 0), 0);
        assert_eq!(char_to_byte("hello", 3), 3);
        assert_eq!(char_to_byte("hello", 5), 5); // past-the-end clamps to len
    }

    #[test]
    fn char_to_byte_handles_multibyte_utf8() {
        // "türkçe" — ü, ç are 2-byte UTF-8 chars, so char index != byte index.
        let s = "türkçe";
        assert_eq!(char_to_byte(s, 0), 0);
        assert_eq!(char_to_byte(s, 1), 1); // 't' is 1 byte
        assert_eq!(char_to_byte(s, 2), 3); // skip 2-byte 'ü'
        assert_eq!(char_to_byte(s, 6), s.len());
    }

    // ─── simple_diff ────────────────────────────────────────────────────────

    #[test]
    fn simple_diff_marks_unchanged_lines_as_context() {
        let diff = simple_diff("a\nb\nc", "a\nb\nc");
        assert_eq!(diff, vec!["  a", "  b", "  c"]);
    }

    #[test]
    fn simple_diff_marks_changed_lines_as_remove_then_add() {
        let diff = simple_diff("a\nb", "a\nx");
        assert_eq!(diff, vec!["  a", "- b", "+ x"]);
    }

    #[test]
    fn simple_diff_handles_added_and_removed_lines() {
        let diff = simple_diff("a\nb", "a");
        assert_eq!(diff, vec!["  a", "- b"]);

        let diff = simple_diff("a", "a\nb");
        assert_eq!(diff, vec!["  a", "+ b"]);
    }

    #[test]
    fn simple_diff_of_empty_strings_is_empty() {
        assert!(simple_diff("", "").is_empty());
    }

    // ─── Editor::from_content / content ─────────────────────────────────────

    #[test]
    fn from_content_splits_into_lines_and_round_trips() {
        let editor = Editor::from_content("a\nb\nc", 10);
        assert_eq!(editor.lines, vec!["a", "b", "c"]);
        assert_eq!(editor.content(), "a\nb\nc");
    }

    #[test]
    fn from_content_of_empty_string_yields_one_empty_line() {
        let editor = Editor::from_content("", 10);
        assert_eq!(editor.lines, vec![""]);
    }

    // ─── Editor::handle_key — insertion / deletion ──────────────────────────

    #[test]
    fn typing_a_char_inserts_at_cursor_and_advances() {
        let mut editor = Editor::from_content("ac", 10);
        editor.cursor_col = 1;
        editor.handle_key(key(KeyCode::Char('b')));
        assert_eq!(editor.lines[0], "abc");
        assert_eq!(editor.cursor_col, 2);
    }

    #[test]
    fn enter_splits_the_current_line_and_moves_cursor_down() {
        let mut editor = Editor::from_content("abcdef", 10);
        editor.cursor_col = 3;
        editor.handle_key(key(KeyCode::Enter));
        assert_eq!(editor.lines, vec!["abc", "def"]);
        assert_eq!(editor.cursor_row, 1);
        assert_eq!(editor.cursor_col, 0);
    }

    #[test]
    fn backspace_within_a_line_removes_the_previous_char() {
        let mut editor = Editor::from_content("abc", 10);
        editor.cursor_col = 2;
        editor.handle_key(key(KeyCode::Backspace));
        assert_eq!(editor.lines[0], "ac");
        assert_eq!(editor.cursor_col, 1);
    }

    #[test]
    fn backspace_at_line_start_joins_with_previous_line() {
        let mut editor = Editor::from_content("ab\ncd", 10);
        editor.cursor_row = 1;
        editor.cursor_col = 0;
        editor.handle_key(key(KeyCode::Backspace));
        assert_eq!(editor.lines, vec!["abcd"]);
        assert_eq!(editor.cursor_row, 0);
        assert_eq!(editor.cursor_col, 2);
    }

    #[test]
    fn backspace_at_very_start_of_document_is_a_no_op() {
        let mut editor = Editor::from_content("abc", 10);
        editor.handle_key(key(KeyCode::Backspace));
        assert_eq!(editor.lines, vec!["abc"]);
        assert_eq!(editor.cursor_col, 0);
    }

    #[test]
    fn delete_within_a_line_removes_the_next_char() {
        let mut editor = Editor::from_content("abc", 10);
        editor.cursor_col = 1;
        editor.handle_key(key(KeyCode::Delete));
        assert_eq!(editor.lines[0], "ac");
        assert_eq!(editor.cursor_col, 1);
    }

    #[test]
    fn delete_at_line_end_joins_with_next_line() {
        let mut editor = Editor::from_content("ab\ncd", 10);
        editor.cursor_col = 2;
        editor.handle_key(key(KeyCode::Delete));
        assert_eq!(editor.lines, vec!["abcd"]);
        assert_eq!(editor.cursor_row, 0);
    }

    // ─── Editor::handle_key — cursor movement ───────────────────────────────

    #[test]
    fn left_and_right_move_within_and_across_lines() {
        let mut editor = Editor::from_content("ab\ncd", 10);
        editor.cursor_row = 1;
        editor.cursor_col = 0;
        editor.handle_key(key(KeyCode::Left));
        assert_eq!((editor.cursor_row, editor.cursor_col), (0, 2));

        editor.handle_key(key(KeyCode::Right));
        assert_eq!((editor.cursor_row, editor.cursor_col), (1, 0));
    }

    #[test]
    fn up_and_down_clamp_column_to_shorter_line_length() {
        let mut editor = Editor::from_content("abcdef\nxy", 10);
        editor.cursor_row = 0;
        editor.cursor_col = 5;
        editor.handle_key(key(KeyCode::Down));
        assert_eq!(editor.cursor_row, 1);
        assert_eq!(editor.cursor_col, 2); // clamped to "xy".len()

        editor.handle_key(key(KeyCode::Up));
        assert_eq!(editor.cursor_row, 0);
        assert_eq!(editor.cursor_col, 2); // stays clamped, doesn't restore 5
    }

    #[test]
    fn home_and_end_jump_to_line_boundaries() {
        let mut editor = Editor::from_content("abcdef", 10);
        editor.cursor_col = 3;
        editor.handle_key(key(KeyCode::End));
        assert_eq!(editor.cursor_col, 6);
        editor.handle_key(key(KeyCode::Home));
        assert_eq!(editor.cursor_col, 0);
    }

    #[test]
    fn unhandled_key_is_a_no_op() {
        let mut editor = Editor::from_content("abc", 10);
        editor.handle_key(key(KeyCode::F(5)));
        editor.handle_key(key(KeyCode::Esc));
        assert_eq!(editor.lines, vec!["abc"]);
        assert_eq!((editor.cursor_row, editor.cursor_col), (0, 0));
    }

    // ─── scroll ──────────────────────────────────────────────────────────────

    #[test]
    fn scroll_follows_cursor_past_the_bottom_of_the_view() {
        let mut editor = Editor::from_content(&"line\n".repeat(10), 3);
        for _ in 0..8 {
            editor.handle_key(key(KeyCode::Down));
        }
        assert_eq!(editor.cursor_row, 8);
        // view_height=3, so scroll must keep the cursor row in [scroll, scroll+3)
        assert!(editor.scroll <= editor.cursor_row);
        assert!(editor.cursor_row < editor.scroll + editor.view_height);
    }
}
