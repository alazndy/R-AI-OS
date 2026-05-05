use crossterm::event::{KeyCode, KeyEvent};

// ─── Simple line editor ───────────────────────────────────────────────────────

pub struct Editor {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll: usize,
    pub view_height: usize,
}

impl Editor {
    pub fn from_content(content: &str, view_height: usize) -> Self {
        let lines: Vec<String> = content.lines().map(str::to_owned).collect();
        let lines = if lines.is_empty() { vec![String::new()] } else { lines };
        Self { lines, cursor_row: 0, cursor_col: 0, scroll: 0, view_height }
    }

    pub fn to_string(&self) -> String {
        self.lines.join("\n")
    }

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
            KeyCode::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    let max = self.lines[self.cursor_row].chars().count();
                    self.cursor_col = self.cursor_col.min(max);
                }
            }
            KeyCode::Down => {
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    let max = self.lines[self.cursor_row].chars().count();
                    self.cursor_col = self.cursor_col.min(max);
                }
            }
            KeyCode::Home => self.cursor_col = 0,
            KeyCode::End => self.cursor_col = self.lines[self.cursor_row].chars().count(),
            KeyCode::PageUp => {
                self.cursor_row = self.cursor_row.saturating_sub(self.view_height);
                self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].chars().count());
            }
            KeyCode::PageDown => {
                self.cursor_row = (self.cursor_row + self.view_height).min(self.lines.len() - 1);
                self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].chars().count());
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

pub fn char_to_byte(s: &str, char_pos: usize) -> usize {
    s.char_indices()
        .nth(char_pos)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}
