use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::mpsc::UnboundedSender;

use super::highlight::{HighlightBatch, HighlightRequest, HighlightWorker, LineSegments};

const HIGHLIGHT_CACHE_MARGIN: usize = 200;

pub struct ContentLoadResult {
    pub load_id: usize,
    pub path: PathBuf,
    pub content: Vec<String>,
    pub archive_rows: Option<Vec<crate::fs::archive::preview::ArchiveTreeRow>>,
    pub large_file: Option<(
        crate::large_text::file_reader::FileReader,
        crate::large_text::line_indexer::LineIndexer,
    )>,
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
    theme_name: Option<String>,
    /// Styled runs for the lines the worker has already highlighted.
    highlighted: HashMap<usize, LineSegments>,
    /// Bumped whenever the open file changes, so results for a file that has
    /// been closed or replaced are recognised as stale and dropped.
    generation: usize,
    highlight_worker: Option<HighlightWorker>,
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

    #[must_use]
    pub fn with_theme_name(name: &str, theme: Option<lumis::themes::Theme>) -> Self {
        Self {
            theme,
            theme_name: Some(name.to_string()),
            ..Self::default()
        }
    }

    /// Attaches the worker that highlights lines off the render thread.
    pub fn set_highlight_worker(&mut self, worker: HighlightWorker) {
        self.highlight_worker = Some(worker);
    }

    /// Asks the worker to compile the current language's queries now, so the
    /// first request for a line does not have to.
    ///
    /// Called once the file is known to be small enough to highlight, which is
    /// before its content has finished loading.
    pub fn warm_highlight(&mut self) {
        let Some(worker) = &self.highlight_worker else {
            return;
        };
        worker.submit(HighlightRequest::Warm {
            language: self.language,
            theme: self.theme_name.clone(),
        });
    }

    /// The styled runs for a line, if the worker has returned them.
    #[must_use]
    pub fn line_segments(&self, idx: usize) -> Option<&[(Option<ratatui::style::Color>, String)]> {
        self.highlighted.get(&idx).map(|segments| &segments[..])
    }

    /// Stores a batch of highlighted lines, ignoring one from a previous file.
    pub fn apply_highlight_batch(&mut self, batch: HighlightBatch) {
        if batch.generation != self.generation {
            return;
        }
        for (index, segments) in batch.lines {
            self.highlighted.insert(index, segments);
        }
    }

    /// Ensures the lines in `start..end` have styled runs, asking the worker for
    /// the ones that do not, and drops runs that have scrolled far away.
    ///
    /// Called from the draw pass, which therefore never waits: a line that is
    /// still missing renders unstyled until its batch arrives.
    pub fn request_highlights(&mut self, start: usize, end: usize) {
        self.prune_highlight_cache(start, end);

        if self.large_file_indexer.is_some() {
            return;
        }

        if self.language == lumis::languages::Language::PlainText {
            for index in start..end {
                if !self.highlighted.contains_key(&index)
                    && let Some(line) = self.get_line(index)
                {
                    self.highlighted.insert(index, vec![(None, line)]);
                }
            }
            return;
        }

        let Some(worker) = &self.highlight_worker else {
            return;
        };
        let missing: Vec<(usize, String)> = (start..end)
            .filter(|index| !self.highlighted.contains_key(index))
            .filter_map(|index| self.get_line(index).map(|line| (index, line)))
            .collect();
        if missing.is_empty() {
            return;
        }
        worker.submit(HighlightRequest::Lines {
            generation: self.generation,
            language: self.language,
            theme: self.theme_name.clone(),
            lines: missing,
        });
    }

    /// Keeps runs for the window the viewer is showing and discards the rest, so
    /// scrolling through a large file does not accumulate them without bound.
    fn prune_highlight_cache(&mut self, start: usize, end: usize) {
        if self.highlighted.is_empty() {
            return;
        }
        let keep_from = start.saturating_sub(HIGHLIGHT_CACHE_MARGIN);
        let keep_to = end + HIGHLIGHT_CACHE_MARGIN;
        self.highlighted
            .retain(|index, _| *index >= keep_from && *index < keep_to);
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
        // Anything the worker still holds belongs to the file being closed, and
        // anything it has already sent would otherwise colour the next one.
        self.highlighted.clear();
        self.generation = self.generation.wrapping_add(1);
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

    /// The worker's runs for a line, when they can stand in for the line's own
    /// character offsets.
    ///
    /// Lumis always covers the whole line, so the runs concatenate back to it;
    /// the length check keeps that assumption from silently turning into a
    /// selection of the wrong text.
    fn syntax_segments(
        &self,
        row: usize,
        char_count: usize,
    ) -> Option<&[(Option<ratatui::style::Color>, String)]> {
        let segments = self.highlighted.get(&row)?;
        if segments.len() < 2 {
            return None;
        }
        let covered: usize = segments.iter().map(|(_, text)| text.chars().count()).sum();
        (covered == char_count).then_some(&segments[..])
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

        // Try syntax-aware selection first, using the runs the highlight worker
        // has already produced for this line.
        if self.large_file_indexer.is_none()
            && let Some(segments) = self.syntax_segments(row, char_count)
        {
            let mut current_char_idx = 0;
            for (_, text) in segments {
                let segment_char_count = text.chars().count();
                if char_idx >= current_char_idx && char_idx < current_char_idx + segment_char_count
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

    /// Whether a line is inside the visible viewport (two lines are reserved
    /// for the bottom bars, matching [`Self::max_scroll_offset`]).
    fn match_visible(&self, line: usize, area: ratatui::layout::Rect) -> bool {
        let visible_lines = area.height.saturating_sub(2) as usize;
        line >= self.scroll_offset && line < self.scroll_offset + visible_lines
    }

    fn jump_to_match_with_context(&mut self, line_idx: usize, area: ratatui::layout::Rect) {
        // If the match is already visible on the current page, don't scroll.
        if self.match_visible(line_idx, area) {
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

        let (start_line, start_char) = match self.current_search_match {
            Some((line, _, end_char)) => {
                // If current match is visible, search after it.
                // Otherwise, search from the current scroll offset.
                if self.match_visible(line, area) {
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

        let (start_line, start_char) = match self.current_search_match {
            Some((line, start_char, _)) => {
                // If current match is visible, search before it.
                // Otherwise, search from the current scroll offset.
                if self.match_visible(line, area) {
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
        let mut found: Option<(usize, usize, usize)> = None;
        if start_line >= total {
            // Start position is past the end (e.g. scrolled past EOF):
            // scan the whole file from the beginning.
            self.for_each_line(0, total, |i, line| {
                if let Some(m) = re.find(line) {
                    let start_c = line[..m.start()].chars().count();
                    let end_c = start_c + line[m.start()..m.end()].chars().count();
                    found = Some((i, start_c, end_c));
                    true
                } else {
                    false
                }
            });
            return found;
        }

        // 1. Current line after start_char
        self.for_each_line(start_line, start_line + 1, |i, line| {
            let byte_idx = line
                .chars()
                .take(start_char)
                .map(char::len_utf8)
                .sum::<usize>();
            if byte_idx < line.len()
                && let Some(m) = re.find(&line[byte_idx..])
            {
                let m_start = byte_idx + m.start();
                let m_end = byte_idx + m.end();
                let start_c = line[..m_start].chars().count();
                let end_c = start_c + line[m_start..m_end].chars().count();
                found = Some((i, start_c, end_c));
                true
            } else {
                false
            }
        });
        if found.is_some() {
            return found;
        }

        // 2. Subsequent lines
        self.for_each_line(start_line + 1, total, |i, line| {
            if let Some(m) = re.find(line) {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                found = Some((i, start_c, end_c));
                true
            } else {
                false
            }
        });
        if found.is_some() {
            return found;
        }

        // 3. Wrap around: 0 to start_line
        self.for_each_line(0, start_line + 1, |i, line| {
            let Some(m) = re.find(line) else {
                return false;
            };
            // Check if this match is before our starting point if it's the same line
            let start_c = line[..m.start()].chars().count();
            if i < start_line || start_c < start_char {
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                found = Some((i, start_c, end_c));
                true
            } else {
                false
            }
        });
        found
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
        let start_line = start_line.min(total - 1);

        // 1. Current line before start_char
        let mut found: Option<(usize, usize, usize)> = None;
        self.for_each_line(start_line, start_line + 1, |i, line| {
            let byte_limit = line
                .chars()
                .take(start_char)
                .map(char::len_utf8)
                .sum::<usize>();
            if byte_limit > 0
                && let Some(m) = re.find_iter(&line[..byte_limit]).last()
            {
                let start_c = line[..m.start()].chars().count();
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                found = Some((i, start_c, end_c));
                true
            } else {
                false
            }
        });
        if found.is_some() {
            return found;
        }

        // 2. Previous lines
        if start_line > 0 {
            self.for_each_line_rev(start_line - 1, 0, |i, line| {
                if let Some(m) = re.find_iter(line).last() {
                    let start_c = line[..m.start()].chars().count();
                    let end_c = start_c + line[m.start()..m.end()].chars().count();
                    found = Some((i, start_c, end_c));
                    true
                } else {
                    false
                }
            });
        }
        if found.is_some() {
            return found;
        }

        // 3. Wrap around: bottom to start_line
        self.for_each_line_rev(total - 1, start_line, |i, line| {
            let Some(m) = re.find_iter(line).last() else {
                return false;
            };
            let start_c = line[..m.start()].chars().count();
            if i > start_line || start_c > start_char {
                let end_c = start_c + line[m.start()..m.end()].chars().count();
                found = Some((i, start_c, end_c));
                true
            } else {
                false
            }
        });
        found
    }

    /// Byte window decoded at once when searching an indexed file.
    const SEARCH_CHUNK_BYTES: usize = 2 * 1024 * 1024;

    /// Calls `f(line_idx, line)` for lines `start_line..end_line` in order and
    /// returns the first line index where `f` returned `true`, if any.
    ///
    /// Small-file lines are borrowed; indexed files are decoded in large
    /// contiguous windows (bounded by the requested line range) instead of
    /// one decode + allocation per line.
    fn for_each_line<F>(&self, start_line: usize, end_line: usize, mut f: F) -> Option<usize>
    where
        F: FnMut(usize, &str) -> bool,
    {
        let total = self.total_lines();
        let start_line = start_line.min(end_line).min(total);
        let end_line = end_line.min(total);
        if start_line >= end_line {
            return None;
        }

        if let (Some(indexer), Some(reader)) = (&self.large_file_indexer, &self.large_file_reader) {
            let offsets = indexer.offsets();
            let file_len = reader.len();
            let mut cursor = start_line;
            while cursor < end_line {
                let byte_start = offsets[cursor];
                // Last line that starts within the decode window.
                let target = byte_start.saturating_add(Self::SEARCH_CHUNK_BYTES);
                let k = match offsets.binary_search(&target) {
                    Ok(j) => j,
                    Err(j) => j.saturating_sub(1),
                }
                .min(end_line);
                let byte_end = if k + 1 < offsets.len() {
                    offsets[k + 1]
                } else {
                    file_len
                };
                let chunk = reader.get_chunk(byte_start, byte_end);
                for (local, line) in chunk.split_terminator('\n').enumerate() {
                    let idx = cursor + local;
                    if idx >= end_line {
                        break;
                    }
                    if f(idx, line) {
                        return Some(idx);
                    }
                }
                cursor = k + 1;
            }
            // Synthetic empty final line (file ends with a newline).
            if end_line == total && offsets.last() == Some(&file_len) && f(total - 1, "") {
                return Some(total - 1);
            }
            None
        } else {
            for i in start_line..end_line {
                if f(i, &self.content[i]) {
                    return Some(i);
                }
            }
            None
        }
    }

    /// Calls `f(line_idx, line)` for lines `start_line, start_line - 1, ...,
    /// end_line` (inclusive) in reverse order.
    fn for_each_line_rev<F>(&self, start_line: usize, end_line: usize, mut f: F) -> Option<usize>
    where
        F: FnMut(usize, &str) -> bool,
    {
        let total = self.total_lines();
        let start_line = start_line.min(total.saturating_sub(1));
        let end_line = end_line.min(total);
        if start_line < end_line {
            return None;
        }

        if let (Some(indexer), Some(reader)) = (&self.large_file_indexer, &self.large_file_reader) {
            let offsets = indexer.offsets();
            let file_len = reader.len();
            let mut cursor = start_line;
            // Synthetic empty final line (file ends with a newline).
            if offsets.last() == Some(&file_len) && cursor == total - 1 {
                if f(total - 1, "") {
                    return Some(total - 1);
                }
                cursor -= 1;
            }
            while cursor >= end_line {
                let byte_end = if cursor + 1 < offsets.len() {
                    offsets[cursor + 1]
                } else {
                    file_len
                };
                // First line that starts at or after the window start.
                let target = byte_end.saturating_sub(Self::SEARCH_CHUNK_BYTES);
                let mut j = match offsets.binary_search(&target) {
                    Ok(x) | Err(x) => x,
                };
                if j > cursor {
                    j = cursor;
                }
                let byte_start = offsets[j];
                let chunk = reader.get_chunk(byte_start, byte_end);
                let lines: Vec<&str> = chunk.split_terminator('\n').collect();
                for (local, line) in lines.iter().enumerate().rev() {
                    let idx = j + local;
                    if idx < end_line {
                        break;
                    }
                    if f(idx, line) {
                        return Some(idx);
                    }
                }
                cursor = j.checked_sub(1)?;
            }
            None
        } else {
            for i in (end_line..=start_line).rev() {
                if f(i, &self.content[i]) {
                    return Some(i);
                }
            }
            None
        }
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
                            large_file: None,
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
                            large_file: None,
                            language: lumis::languages::Language::default(),
                        }
                    }
                }
                Err(e) => ContentLoadResult {
                    load_id,
                    path: result_path,
                    content: vec![format!("Error scanning archive: {e}")],
                    archive_rows: None,
                    large_file: None,
                    language: lumis::languages::Language::default(),
                },
            };
            let _ = tx.send(load_result);
        });
    }

    /// Indexes a large local file in a blocking task so the UI event loop is not blocked.
    ///
    /// The result (reader + line index) is delivered via the content load channel; stale
    /// results are dropped via `load_id`/path checks and the cancel flag.
    pub fn spawn_large_file_index(
        &mut self,
        path: &std::path::Path,
        encoding: &'static encoding_rs::Encoding,
        size: u64,
        limit_bytes: u64,
        cancel_flag: Arc<AtomicBool>,
    ) {
        self.content_load_id += 1;
        let load_id = self.content_load_id;
        let path = path.to_path_buf();
        let Some(tx) = self.content_load_tx.clone() else {
            return;
        };
        tokio::spawn(async move {
            let result_path = path.clone();
            let index = tokio::task::spawn_blocking(move || {
                let reader = crate::large_text::file_reader::FileReader::new(path, encoding)?;
                let mut indexer = crate::large_text::line_indexer::LineIndexer::new();
                indexer.index_file(&reader);
                Ok::<_, anyhow::Error>((reader, indexer))
            })
            .await
            .unwrap_or_else(|e| Err(anyhow::anyhow!(e.to_string())));

            if cancel_flag.load(Ordering::Relaxed) {
                return;
            }

            let load_result = match index {
                Ok((reader, indexer)) => ContentLoadResult {
                    load_id,
                    path: result_path,
                    content: Vec::new(),
                    archive_rows: None,
                    large_file: Some((reader, indexer)),
                    language: lumis::languages::Language::default(),
                },
                Err(e) => ContentLoadResult {
                    load_id,
                    path: result_path,
                    content: vec![
                        format!(
                            "File too large to display (size: {}, limit: {})",
                            crate::fs::utils::format_size(Some(size), false, false),
                            crate::fs::utils::format_size(Some(limit_bytes), false, false)
                        ),
                        format!("Indexing failed: {e}"),
                    ],
                    archive_rows: None,
                    large_file: None,
                    language: lumis::languages::Language::default(),
                },
            };
            let _ = tx.send(load_result);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(generation: usize, lines: Vec<(usize, LineSegments)>) -> crate::state::HighlightBatch {
        crate::state::HighlightBatch { generation, lines }
    }

    #[test]
    fn stale_generations_are_dropped() {
        let mut state = TextViewerState::default();
        state.apply_highlight_batch(batch(0, vec![(0, vec![(None, "kept".to_string())])]));
        assert_eq!(state.line_segments(0).map(<[_]>::len), Some(1));

        // A new file bumps the generation, so results for the old one no longer
        // apply and a late batch must not colour the new one.
        state.reset();
        state.apply_highlight_batch(batch(0, vec![(0, vec![(None, "stale".to_string())])]));
        assert!(state.line_segments(0).is_none(), "stale batch was applied");

        state.apply_highlight_batch(batch(1, vec![(0, vec![(None, "fresh".to_string())])]));
        assert_eq!(
            state.line_segments(0).map(|runs| runs[0].1.as_str()),
            Some("fresh")
        );
    }

    #[test]
    fn plain_text_lines_need_no_worker() {
        let mut state = TextViewerState {
            content: vec!["one".to_string(), "two".to_string(), "three".to_string()],
            ..Default::default()
        };

        // No worker attached: plain text must still resolve to runs, or the draw
        // pass would keep asking for them.
        state.request_highlights(0, 2);

        assert_eq!(
            state.line_segments(0).map(|runs| runs[0].1.as_str()),
            Some("one")
        );
        assert_eq!(
            state.line_segments(1).map(|runs| runs[0].1.as_str()),
            Some("two")
        );
        assert!(state.line_segments(2).is_none(), "outside the window");
    }

    #[test]
    fn cache_is_pruned_around_the_viewport() {
        let mut state = TextViewerState {
            content: (0..5000).map(|i| i.to_string()).collect(),
            ..Default::default()
        };

        state.request_highlights(0, 10);
        assert!(state.line_segments(0).is_some());

        // Jump far away; the old runs must not be kept forever.
        state.request_highlights(4000, 4010);
        assert!(state.line_segments(0).is_none(), "stale run was retained");
        assert!(state.line_segments(4005).is_some());
    }

    #[test]
    fn syntax_selection_follows_the_cached_runs() {
        let mut state = TextViewerState {
            content: vec!["a + b".to_string()],
            ..Default::default()
        };
        state.apply_highlight_batch(batch(
            0,
            vec![(
                0,
                vec![
                    (None, "a".to_string()),
                    (None, " ".to_string()),
                    (None, "+".to_string()),
                    (None, " ".to_string()),
                    (None, "b".to_string()),
                ],
            )],
        ));

        // Clicking the operator selects just the operator, which only the
        // syntax runs can do: word boundaries would take the spaces too.
        state.select_word_at(0, 2);
        assert_eq!(state.selection, Some(((0, 2), (0, 3))));
    }
}
