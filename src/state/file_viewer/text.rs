use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::UnboundedSender;

pub struct ContentLoadResult {
    pub load_id: usize,
    pub path: PathBuf,
    pub content: Vec<String>,
    pub archive_rows: Option<Vec<crate::fs::archive::preview::ArchiveTreeRow>>,
    pub language: lumis::languages::Language,
}

/// State for the text/archive side of the file viewer.
#[derive(Default)]
pub struct TextViewerState {
    pub content: Vec<String>,
    pub scroll_offset: usize,
    pub horizontal_scroll_offset: usize,
    pub language: lumis::languages::Language,
    pub theme: Option<lumis::themes::Theme>,
    pub selection: Option<((usize, usize), (usize, usize))>,
    pub large_file_reader: Option<crate::large_text::file_reader::FileReader>,
    pub large_file_indexer: Option<crate::large_text::line_indexer::LineIndexer>,
    pub archive_rows: Option<Vec<crate::fs::archive::preview::ArchiveTreeRow>>,
    pub current_search_match: Option<(usize, usize, usize)>, // (line_idx, start_char, end_char)
    search_query: String,
    search_regex: Option<regex::Regex>,
    content_load_tx: Option<UnboundedSender<ContentLoadResult>>,
    pub(super) content_load_id: usize,
}

impl TextViewerState {
    #[must_use]
    pub fn with_theme(theme: Option<lumis::themes::Theme>) -> Self {
        Self {
            theme,
            ..Self::default()
        }
    }

    /// The active search query, if any.
    #[must_use]
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn set_content_load_channel(&mut self, tx: UnboundedSender<ContentLoadResult>) {
        self.content_load_tx = Some(tx);
    }

    pub fn reset(&mut self) {
        self.content = Vec::new();
        self.scroll_offset = 0;
        self.horizontal_scroll_offset = 0;
        self.language = lumis::languages::Language::default();
        self.selection = None;
        self.large_file_reader = None;
        self.large_file_indexer = None;
        self.search_query = String::new();
        self.search_regex = None;
        self.current_search_match = None;
        self.archive_rows = None;
    }

    #[must_use]
    pub fn total_lines(&self) -> usize {
        if let Some(indexer) = &self.large_file_indexer {
            indexer.total_lines()
        } else {
            self.content.len()
        }
    }

    #[must_use]
    pub fn get_line(&self, idx: usize) -> Option<String> {
        if let Some(indexer) = &self.large_file_indexer
            && let Some(reader) = &self.large_file_reader
        {
            return indexer
                .get_line_with_reader(idx, reader)
                .map(|(s, e)| reader.get_chunk(s, e));
        }
        self.content.get(idx).cloned()
    }

    #[must_use]
    pub fn get_selected_text(&self) -> Option<String> {
        let ((r1, c1), (r2, c2)) = self.selection?;
        let (start_r, start_c, end_r, end_c) = if r1 < r2 || (r1 == r2 && c1 <= c2) {
            (r1, c1, r2, c2)
        } else {
            (r2, c2, r1, c1)
        };

        let mut selected_lines = Vec::new();
        for r in start_r..=end_r {
            let line_opt = if let Some(indexer) = &self.large_file_indexer {
                self.large_file_reader.as_ref().and_then(|reader| {
                    indexer
                        .get_line_with_reader(r, reader)
                        .map(|(s, e)| reader.get_chunk(s, e))
                })
            } else {
                self.content.get(r).cloned()
            };

            if let Some(line) = line_opt {
                if start_r == end_r {
                    // Selection is within a single line
                    let s = line
                        .chars()
                        .skip(start_c)
                        .take(end_c.saturating_sub(start_c))
                        .collect::<String>();
                    selected_lines.push(s);
                } else if r == start_r {
                    // First line of multi-line selection
                    let s = line.chars().skip(start_c).collect::<String>();
                    selected_lines.push(s);
                } else if r == end_r {
                    // Last line of multi-line selection
                    let s = line.chars().take(end_c).collect::<String>();
                    selected_lines.push(s);
                } else {
                    // Intermediate lines
                    selected_lines.push(line.clone());
                }
            }
        }

        if selected_lines.is_empty() {
            None
        } else {
            Some(selected_lines.join("\n"))
        }
    }

    /// Converts a display column to a character index for a given row.
    /// Handles tab expansion (4 spaces) and wide characters.
    #[must_use]
    pub fn display_col_to_char_idx(&self, row: usize, display_col: usize) -> usize {
        let line_opt = if let Some(indexer) = &self.large_file_indexer {
            self.large_file_reader.as_ref().and_then(|reader| {
                indexer
                    .get_line_with_reader(row, reader)
                    .map(|(s, e)| reader.get_chunk(s, e))
            })
        } else {
            self.content.get(row).cloned()
        };

        let Some(line) = line_opt else {
            return display_col;
        };

        let mut current_display_pos = 0;
        for (idx, ch) in line.chars().enumerate() {
            let ch_width = if ch == '\t' {
                4 - (current_display_pos % 4)
            } else {
                unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1)
            };

            if current_display_pos + ch_width > display_col {
                return idx;
            }
            current_display_pos += ch_width;
        }
        line.chars().count()
    }

    /// Selects the word or syntax chunk at the given display coordinates.
    pub fn select_word_at(&mut self, row: usize, display_col: usize) {
        let line_opt = if let Some(indexer) = &self.large_file_indexer {
            self.large_file_reader.as_ref().and_then(|reader| {
                indexer
                    .get_line_with_reader(row, reader)
                    .map(|(s, e)| reader.get_chunk(s, e))
            })
        } else {
            self.content.get(row).cloned()
        };

        let Some(line) = line_opt else {
            self.selection = None;
            return;
        };

        let char_idx = self.display_col_to_char_idx(row, display_col);
        let char_count = line.chars().count();
        if char_idx >= char_count {
            self.selection = None;
            return;
        }

        let mut start = 0;
        let mut end = 0;
        let mut found = false;

        // Try syntax-aware selection first
        if self.large_file_indexer.is_none() {
            let highlighter = lumis::highlight::Highlighter::new(self.language, self.theme.clone());
            let segments = highlighter.highlight(&line).unwrap_or_default();

            if segments.len() > 1 {
                let mut current_char_idx = 0;
                for (_, text) in segments {
                    let segment_char_count = text.chars().count();
                    if char_idx >= current_char_idx
                        && char_idx < current_char_idx + segment_char_count
                    {
                        // Found the syntax chunk
                        start = current_char_idx;
                        end = current_char_idx + segment_char_count;
                        found = true;
                        break;
                    }
                    current_char_idx += segment_char_count;
                }
            }
        }

        let chars: Vec<char> = line.chars().collect();
        if !found {
            // Fallback: word boundaries (alphanumeric + underscore)
            let is_word_char = |c: char| c.is_alphanumeric() || c == '_';

            let target_is_word = is_word_char(chars[char_idx]);

            start = char_idx;
            while start > 0 && is_word_char(chars[start - 1]) == target_is_word {
                start -= 1;
            }

            end = char_idx;
            while end < char_count && is_word_char(chars[end]) == target_is_word {
                end += 1;
            }
        }

        // Refine selection to exclude surrounding quotes if present (supports triple and nested quotes)
        while end - start >= 2 {
            let s_char = chars[start];
            let e_char = chars[end - 1];
            if (s_char == '"' && e_char == '"')
                || (s_char == '\'' && e_char == '\'')
                || (s_char == '`' && e_char == '`')
            {
                start += 1;
                end -= 1;
            } else {
                break;
            }
        }

        self.selection = Some(((row, start), (row, end)));
    }

    pub fn search(&mut self, query: &str, area: ratatui::layout::Rect) -> bool {
        if query.is_empty() {
            self.search_query = String::new();
            self.search_regex = None;
            self.current_search_match = None;
            return false;
        }

        let Ok(re) = regex::RegexBuilder::new(query)
            .case_insensitive(true)
            .build()
        else {
            return false;
        };

        self.search_query = query.to_string();
        self.search_regex = Some(re.clone());

        // Find first match at or after current scroll position
        if let Some(m) = self.find_next_match(self.scroll_offset, 0, &re) {
            self.current_search_match = Some(m);
            self.jump_to_match_with_context(m.0, area);
            true
        } else {
            self.current_search_match = None;
            false
        }
    }

    fn jump_to_match_with_context(&mut self, line_idx: usize, area: ratatui::layout::Rect) {
        let viewport_height = area.height as usize;
        // If the match is already visible on the current page, don't scroll.
        if line_idx >= self.scroll_offset
            && line_idx < self.scroll_offset + viewport_height.saturating_sub(1)
        {
            return;
        }
        // Leave 4 lines above for context, clamped so we never scroll past the content.
        self.scroll_offset = line_idx.saturating_sub(4).min(self.max_scroll_offset(area));
    }

    fn max_scroll_offset(&self, area: ratatui::layout::Rect) -> usize {
        self.total_lines()
            .saturating_sub(area.height.saturating_sub(2) as usize)
    }

    pub fn search_next(&mut self, area: ratatui::layout::Rect) -> bool {
        let Some(re) = self.search_regex.clone() else {
            return false;
        };

        let viewport_height = area.height as usize;
        let (start_line, start_char) = match self.current_search_match {
            Some((line, _, end_char)) => {
                // If current match is visible, search after it.
                // Otherwise, search from the current scroll offset.
                if line >= self.scroll_offset && line < self.scroll_offset + viewport_height {
                    (line, end_char)
                } else {
                    (self.scroll_offset, 0)
                }
            }
            None => (self.scroll_offset, 0),
        };

        if let Some(m) = self.find_next_match(start_line, start_char, &re) {
            if Some(m) == self.current_search_match {
                return false;
            }
            self.current_search_match = Some(m);
            self.jump_to_match_with_context(m.0, area);
            true
        } else {
            false
        }
    }

    pub fn search_prev(&mut self, area: ratatui::layout::Rect) -> bool {
        let Some(re) = self.search_regex.clone() else {
            return false;
        };

        let viewport_height = area.height as usize;
        let (start_line, start_char) = match self.current_search_match {
            Some((line, start_char, _)) => {
                // If current match is visible, search before it.
                // Otherwise, search from the current scroll offset.
                if line >= self.scroll_offset && line < self.scroll_offset + viewport_height {
                    (line, start_char)
                } else {
                    (self.scroll_offset, 0)
                }
            }
            None => (self.scroll_offset, 0),
        };

        if let Some(m) = self.find_prev_match(start_line, start_char, &re) {
            if Some(m) == self.current_search_match {
                return false;
            }
            self.current_search_match = Some(m);
            self.jump_to_match_with_context(m.0, area);
            true
        } else {
            false
        }
    }

    fn find_next_match(
        &self,
        start_line: usize,
        start_char: usize,
        re: &regex::Regex,
    ) -> Option<(usize, usize, usize)> {
        let total = self.total_lines();
        if total == 0 {
            return None;
        }

        // 1. Current line after start_char
        if let Some(line) = self.get_line(start_line) {
            let byte_idx = line.chars().take(start_char).map(char::len_utf8).sum();
            if byte_idx < line.len()
                && let Some(m) = re.find(&line[byte_idx..])
            {
                let m_start = byte_idx + m.start();
                let m_end = byte_idx + m.end();
                let start_c = line[..m_start].chars().count();
                let end_c = start_c + line[m_start..m_end].chars().count();
                return Some((start_line, start_c, end_c));
            }
        }

        // 2. Subsequent lines
        for i in (start_line + 1)..total {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find(&line)
            {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                return Some((i, start_c, end_c));
            }
        }

        // 3. Wrap around: 0 to start_line
        for i in 0..=start_line {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find(&line)
            {
                // Check if this match is before our starting point if it's the same line
                let m_start_byte = m.start();
                let start_c = line[..m_start_byte].chars().count();

                if i < start_line || start_c < start_char {
                    let end_c = start_c + line[m_start_byte..m.end()].chars().count();
                    return Some((i, start_c, end_c));
                }
            }
        }

        None
    }

    fn find_prev_match(
        &self,
        start_line: usize,
        start_char: usize,
        re: &regex::Regex,
    ) -> Option<(usize, usize, usize)> {
        let total = self.total_lines();
        if total == 0 {
            return None;
        }

        // 1. Current line before start_char
        if let Some(line) = self.get_line(start_line) {
            let byte_limit = line.chars().take(start_char).map(char::len_utf8).sum();
            if byte_limit > 0
                && let Some(m) = re.find_iter(&line[..byte_limit]).last()
            {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                return Some((start_line, start_c, end_c));
            }
        }

        // 2. Previous lines
        for i in (0..start_line).rev() {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find_iter(&line).last()
            {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                return Some((i, start_c, end_c));
            }
        }

        // 3. Wrap around: bottom to start_line
        for i in (start_line..total).rev() {
            if let Some(line) = self.get_line(i)
                && let Some(m) = re.find_iter(&line).last()
            {
                let m_start_byte = m.start();
                let start_c = line[..m_start_byte].chars().count();

                if i > start_line || start_c > start_char {
                    let end_c = start_c + line[m_start_byte..m.end()].chars().count();
                    return Some((i, start_c, end_c));
                }
            }
        }

        None
    }

    pub fn spawn_archive_scan(&mut self, path: &std::path::Path, cancel_flag: Arc<AtomicBool>) {
        self.content_load_id += 1;
        let load_id = self.content_load_id;
        let path = path.to_path_buf();
        let Some(tx) = self.content_load_tx.clone() else {
            return;
        };
        tokio::spawn(async move {
            let result_path = path.clone();
            let result = tokio::task::spawn_blocking(move || {
                let handler = crate::fs::archive::get_archive_handler(&path)?;
                handler.scan()
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!(e.to_string())));
            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }
            let load_result = match result {
                Ok(scan_result) => {
                    let rows = crate::fs::archive::preview::build_archive_tree(&scan_result);
                    if rows.is_empty() {
                        ContentLoadResult {
                            load_id,
                            path: result_path,
                            content: Vec::new(),
                            archive_rows: None,
                            language: lumis::languages::Language::default(),
                        }
                    } else {
                        let content = rows
                            .iter()
                            .map(|r| format!("{}{}", r.prefix, r.name))
                            .collect();
                        ContentLoadResult {
                            load_id,
                            path: result_path,
                            content,
                            archive_rows: Some(rows),
                            language: lumis::languages::Language::default(),
                        }
                    }
                }
                Err(e) => ContentLoadResult {
                    load_id,
                    path: result_path,
                    content: vec![format!("Error scanning archive: {e}")],
                    archive_rows: None,
                    language: lumis::languages::Language::default(),
                },
            };
            let _ = tx.send(load_result);
        });
    }
}
