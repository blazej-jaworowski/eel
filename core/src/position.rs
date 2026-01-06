/// Represents a coordinate location within a buffer.
///
/// This struct uses a 0-indexed coordinate system where `row` corresponds to the vertical
/// line number and `col` corresponds to the horizontal character index.
///
/// # Coordinate System
///
/// * **(0, 0)**: Represents the top-left corner of the buffer (first character of the first line).
/// * **Row**: Increases moving downwards.
/// * **Col**: Increases moving to the right.
///
/// # Bounds
///
/// The `col` index may extend to `row_length` to represent a cursor position
/// located immediately after the last character of the line.
/// `col` on an empty line will always be 0.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

impl Position {
    pub const fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    pub const fn origin() -> Self {
        Self::new(0, 0)
    }

    pub const fn next_col(mut self) -> Self {
        self.col += 1;
        self
    }

    pub const fn next_row(mut self) -> Self {
        self.row += 1;
        self
    }

    pub const fn prev_col(mut self) -> Self {
        self.col = self.col.saturating_sub(1);
        self
    }

    pub const fn prev_row(mut self) -> Self {
        self.row = self.row.saturating_sub(1);
        self
    }

    pub fn max_text_pos(text: &str) -> Self {
        if text.is_empty() {
            return Self::new(0, 0);
        }

        let mut line_count = 0;
        let mut last_line: Option<&str> = None;

        let mut lines = text.split("\n").peekable();
        while let Some(line) = lines.next() {
            line_count += 1;
            if lines.peek().is_none() {
                last_line = Some(line);
            }
        }

        let last_line = last_line.unwrap_or(text);

        Position::new(line_count - 1, last_line.len())
    }

    pub fn offset(&self, by: &Position) -> Self {
        if by.row == 0 {
            Self::new(self.row, self.col + by.col)
        } else {
            Self::new(self.row + by.row, by.col)
        }
    }
}

impl From<(usize, usize)> for Position {
    fn from((row, col): (usize, usize)) -> Self {
        Position { row, col }
    }
}

impl From<Position> for (usize, usize) {
    fn from(position: Position) -> Self {
        (position.row, position.col)
    }
}
