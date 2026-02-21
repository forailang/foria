use lsp_types::Position;

pub struct LineIndex {
    line_starts: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        LineIndex {
            line_starts,
            len: text.len(),
        }
    }

    pub fn offset_to_position(&self, offset: usize) -> Position {
        let offset = offset.min(self.len);
        let line = match self.line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(line) => line - 1,
        };
        let col = offset - self.line_starts[line];
        Position {
            line: line as u32,
            character: col as u32,
        }
    }

    pub fn position_to_offset(&self, pos: Position) -> usize {
        let line = pos.line as usize;
        if line >= self.line_starts.len() {
            return self.len;
        }
        let base = self.line_starts[line];
        let col = pos.character as usize;
        (base + col).min(self.len)
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }
}
