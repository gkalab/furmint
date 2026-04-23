use crate::large_text::file_reader::FileReader;

pub struct LineIndexer {
    line_offsets: Vec<usize>,
    total_lines: usize,
}

impl Default for LineIndexer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineIndexer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            line_offsets: vec![0],
            total_lines: 0,
        }
    }

    /// Indexes the file provided by the `FileReader` by scanning for newlines.
    ///
    /// # Panics
    ///
    /// This function should not panic under normal circumstances.
    pub fn index_file(&mut self, reader: &FileReader) {
        self.line_offsets.clear();
        self.line_offsets.push(0);

        let data = reader.all_data();
        for (i, &byte) in data.iter().enumerate() {
            if byte == b'\n' {
                self.line_offsets.push(i + 1);
            }
        }

        // If the file doesn't end with a newline, we still have the last line indexed.
        // If it does end with a newline, the last offset will be the file size (empty line at EOF).
        self.total_lines = self.line_offsets.len();

        // Remove the trailing empty line if the file ends with a newline
        // (to match standard lines() behavior if desired, but here we keep it to show the empty line)
        if self
            .line_offsets
            .last()
            .is_some_and(|&last| last == reader.len())
        {
            // Optional: self.total_lines -= 1;
        }
    }

    #[must_use]
    pub fn get_line_range(&self, line_num: usize) -> Option<(usize, usize)> {
        if line_num >= self.line_offsets.len() {
            return None;
        }

        let start = self.line_offsets[line_num];
        let end = if line_num + 1 < self.line_offsets.len() {
            self.line_offsets[line_num + 1]
        } else {
            usize::MAX
        };

        Some((start, end))
    }

    #[must_use]
    pub fn get_line_with_reader(
        &self,
        line_num: usize,
        _reader: &FileReader,
    ) -> Option<(usize, usize)> {
        self.get_line_range(line_num)
    }

    #[must_use]
    pub fn total_lines(&self) -> usize {
        self.total_lines
    }
}
