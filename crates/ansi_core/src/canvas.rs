use std::fmt;

pub const MAX_CELLS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Ansi(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attributes {
    pub bold: bool,
    pub blink: bool,
    pub inverse: bool,
    pub underline: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attributes,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: Color::Ansi(7),
            bg: Color::Ansi(0),
            attrs: Attributes::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DimensionError;

impl fmt::Display for DimensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "dimensions must be nonzero and contain at most {MAX_CELLS} cells"
        )
    }
}
impl std::error::Error for DimensionError {}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Result<Self, DimensionError> {
        let count = width
            .checked_mul(height)
            .filter(|n| width > 0 && height > 0 && *n <= MAX_CELLS)
            .ok_or(DimensionError)?;
        Ok(Self {
            width,
            height,
            cells: vec![Cell::default(); count],
        })
    }
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn get(&self, x: usize, y: usize) -> Option<&Cell> {
        if x < self.width && y < self.height {
            Some(&self.cells[y * self.width + x])
        } else {
            None
        }
    }
    pub fn set(&mut self, x: usize, y: usize, cell: Cell) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.cells[y * self.width + x] = cell;
        true
    }
    pub fn clear(&mut self, cell: Cell) {
        self.cells.fill(cell);
    }
    pub fn resize(&mut self, width: usize, height: usize) -> Result<(), DimensionError> {
        let mut next = Self::new(width, height)?;
        self.blit(&mut next, 0, 0);
        *self = next;
        Ok(())
    }
    pub fn blit(&self, target: &mut Self, x: usize, y: usize) {
        for sy in 0..self.height.min(target.height.saturating_sub(y)) {
            for sx in 0..self.width.min(target.width.saturating_sub(x)) {
                target.set(x + sx, y + sy, *self.get(sx, sy).unwrap());
            }
        }
    }
    pub fn fill(&mut self, x: usize, y: usize, replacement: Cell) {
        let Some(&original) = self.get(x, y) else {
            return;
        };
        if original == replacement {
            return;
        }
        let mut pending = vec![(x, y)];
        while let Some((x, y)) = pending.pop() {
            if self.get(x, y) != Some(&original) {
                continue;
            }
            self.set(x, y, replacement);
            if x > 0 {
                pending.push((x - 1, y));
            }
            if x + 1 < self.width {
                pending.push((x + 1, y));
            }
            if y > 0 {
                pending.push((x, y - 1));
            }
            if y + 1 < self.height {
                pending.push((x, y + 1));
            }
        }
    }
}

#[derive(Default)]
pub struct History {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

enum Edit {
    Paint(Vec<(usize, usize, Cell, Cell)>),
    Resize(Canvas),
}

impl History {
    pub fn paint(&mut self, canvas: &mut Canvas, x: usize, y: usize, cell: Cell) -> bool {
        self.paint_many(canvas, [(x, y, cell)])
    }

    pub fn paint_many(
        &mut self,
        canvas: &mut Canvas,
        cells: impl IntoIterator<Item = (usize, usize, Cell)>,
    ) -> bool {
        let mut changes = Vec::new();
        for (x, y, cell) in cells {
            if let Some(&old) = canvas.get(x, y) {
                if old != cell {
                    canvas.set(x, y, cell);
                    changes.push((x, y, old, cell));
                }
            }
        }
        if changes.is_empty() {
            return false;
        }
        self.undo.push(Edit::Paint(changes));
        self.redo.clear();
        true
    }
    pub fn resize(
        &mut self,
        canvas: &mut Canvas,
        width: usize,
        height: usize,
    ) -> Result<bool, DimensionError> {
        if canvas.width() == width && canvas.height() == height {
            return Ok(false);
        }
        let mut next = canvas.clone();
        next.resize(width, height)?;
        let previous = std::mem::replace(canvas, next);
        self.undo.push(Edit::Resize(previous));
        self.redo.clear();
        Ok(true)
    }
    pub fn undo(&mut self, canvas: &mut Canvas) -> bool {
        if let Some(mut edit) = self.undo.pop() {
            match &mut edit {
                Edit::Paint(changes) => {
                    for &(x, y, old, _) in changes.iter().rev() {
                        canvas.set(x, y, old);
                    }
                }
                Edit::Resize(previous) => std::mem::swap(canvas, previous),
            }
            self.redo.push(edit);
            return true;
        }
        false
    }
    pub fn redo(&mut self, canvas: &mut Canvas) -> bool {
        if let Some(mut edit) = self.redo.pop() {
            match &mut edit {
                Edit::Paint(changes) => {
                    for &(x, y, _, new) in changes.iter() {
                        canvas.set(x, y, new);
                    }
                }
                Edit::Resize(next) => std::mem::swap(canvas, next),
            }
            self.undo.push(edit);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_resize_fill_and_history() {
        assert!(Canvas::new(0, 1).is_err());
        assert!(Canvas::new(usize::MAX, 2).is_err());
        let mut c = Canvas::new(2, 2).unwrap();
        let mut h = History::default();
        let cell = Cell {
            ch: 'X',
            ..Cell::default()
        };
        h.paint(&mut c, 1, 1, cell);
        h.undo(&mut c);
        assert_eq!(c.get(1, 1).unwrap().ch, ' ');
        h.redo(&mut c);
        c.resize(3, 3).unwrap();
        assert_eq!(c.get(1, 1).unwrap().ch, 'X');
        c.fill(0, 0, cell);
        assert_eq!(c.get(2, 2).unwrap().ch, 'X');
    }

    #[test]
    fn undo_redo_resize_restores_cropped_content() {
        let mut canvas = Canvas::new(2, 1).unwrap();
        let mut history = History::default();
        history.paint(
            &mut canvas,
            1,
            0,
            Cell {
                ch: 'X',
                ..Cell::default()
            },
        );
        history.resize(&mut canvas, 1, 1).unwrap();
        assert_eq!(canvas.width(), 1);
        history.undo(&mut canvas);
        assert_eq!(canvas.get(1, 0).unwrap().ch, 'X');
        history.undo(&mut canvas);
        assert_eq!(canvas.get(1, 0).unwrap().ch, ' ');
        history.redo(&mut canvas);
        history.redo(&mut canvas);
        assert_eq!(canvas.width(), 1);
    }
}
