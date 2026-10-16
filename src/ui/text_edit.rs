//! A reusable single-line text edit field for popups.
//!
//! The module pairs an editing state ([`TextEditState`]) with a ratatui
//! [`Widget`] ([`TextEditWidget`]) that renders it with selection highlighting,
//! horizontal scrolling and an optional password mask.
//!
//! The state stores the caret as a **char** index,
//! matching the rest of the UI's text fields, and keeps all string mutations
//! char-safe. Key handling ([`TextEditState::handle_key`]) implements the usual
//! editing keys (caret movement, word jumps, shift-selection, copy/cut/paste,
//! word deletion); mouse helpers turn a click column into a caret position and
//! support click-drag selection.

use crate::handlers::clipboard_utils;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use secrecy::SecretString;
use termina::event::{KeyCode, Modifiers};
use zeroize::{Zeroize, Zeroizing};

/// A character that continues a word for word-wise movement and deletion.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Start of the word at or before `from` (skipping non-word chars first).
fn prev_word_boundary(chars: &[char], from: usize) -> usize {
    let mut i = from.min(chars.len());
    while i > 0 && !is_word_char(chars[i - 1]) {
        i -= 1;
    }
    while i > 0 && is_word_char(chars[i - 1]) {
        i -= 1;
    }
    i
}

/// End of the word after `from` (skipping non-word chars first).
fn next_word_boundary(chars: &[char], from: usize) -> usize {
    let n = chars.len();
    let mut i = from.min(n);
    while i < n && !is_word_char(chars[i]) {
        i += 1;
    }
    while i < n && is_word_char(chars[i]) {
        i += 1;
    }
    i
}

/// The `[start, end)` char range of the word containing (or adjacent to) `idx`.
fn word_range(chars: &[char], idx: usize) -> (usize, usize) {
    let n = chars.len();
    let anchor = if idx < n && is_word_char(chars[idx]) {
        idx
    } else if idx > 0 && is_word_char(chars[idx - 1]) {
        idx - 1
    } else {
        return (idx.min(n), idx.min(n));
    };
    let mut start = anchor;
    while start > 0 && is_word_char(chars[start - 1]) {
        start -= 1;
    }
    let mut end = anchor;
    while end < n && is_word_char(chars[end]) {
        end += 1;
    }
    (start, end)
}

/// The horizontal scroll offset (in chars) that keeps `cursor` visible within
/// `width` columns.
#[must_use]
pub fn scroll_offset(cursor: usize, width: usize) -> usize {
    if width == 0 || cursor < width {
        0
    } else {
        cursor - width + 1
    }
}

/// Editing state for a single-line text field.
///
/// The caret and selection anchor are char indices. A selection is the range
/// between the anchor and the caret; it is empty when the two coincide.
#[derive(Clone, Default)]
pub struct TextEditState {
    text: Zeroizing<String>,
    cursor: usize,
    anchor: Option<usize>,
}

impl TextEditState {
    /// Creates an empty field.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a field containing `text` with the caret at the end.
    #[must_use]
    pub fn with_text(text: impl Into<String>) -> Self {
        let mut state = Self::new();
        state.set_text(text);
        state
    }

    /// The current text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Number of characters in the field.
    #[must_use]
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Whether the field is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The caret position (char index).
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Replaces the contents, moving the caret to the end and clearing the
    /// selection.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = Zeroizing::new(text.into());
        self.cursor = self.len();
        self.anchor = None;
    }

    /// Empties the field.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
    }

    /// The active selection as a char range, or `None` when nothing is selected.
    #[must_use]
    pub fn selection(&self) -> Option<std::ops::Range<usize>> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some(anchor.min(self.cursor)..anchor.max(self.cursor))
    }

    /// The selected characters, if any.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let range = self.selection()?;
        Some(
            self.text
                .chars()
                .skip(range.start)
                .take(range.end - range.start)
                .collect(),
        )
    }

    /// Selects the whole field.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.len();
    }

    /// Clears the selection, keeping the caret where it is.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Moves the caret to `pos`, clearing any selection.
    pub fn set_cursor(&mut self, pos: usize) {
        self.cursor = pos.min(self.len());
        self.anchor = None;
    }

    /// Extends the selection to `pos` (anchoring at the caret when starting).
    pub fn extend_selection(&mut self, pos: usize) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = pos.min(self.len());
    }

    /// Maps a click `column` (0-based, relative to the text area) to a caret
    /// position, taking the current scroll offset into account.
    #[must_use]
    pub fn cursor_from_column(&self, column: usize, width: usize) -> usize {
        let scroll = scroll_offset(self.cursor, width);
        column.saturating_add(scroll).min(self.len())
    }

    /// Selects the word at `column` (used by double-click).
    pub fn select_word_at(&mut self, column: usize, width: usize) {
        let idx = self.cursor_from_column(column, width);
        let chars: Vec<char> = self.text.chars().collect();
        let (start, end) = word_range(&chars, idx);
        self.cursor = end;
        self.anchor = Some(start);
    }

    /// Handles one key press, returning whether it was consumed.
    ///
    /// `numeric` restricts typed characters (and pasted text) to ASCII digits.
    #[must_use]
    pub fn handle_key(&mut self, code: KeyCode, modifiers: Modifiers, numeric: bool) -> bool {
        let ctrl = modifiers.contains(Modifiers::CONTROL);
        let shift = modifiers.contains(Modifiers::SHIFT);
        let alt = modifiers.contains(Modifiers::ALT);
        match code {
            KeyCode::Backspace if ctrl => {
                self.delete_word_back();
            }
            KeyCode::Backspace => {
                self.backspace();
            }
            KeyCode::Delete if ctrl => {
                self.delete_word_forward();
            }
            KeyCode::Delete => {
                self.delete_forward();
            }
            KeyCode::Left if ctrl => self.move_word_left(shift),
            KeyCode::Left => self.move_left(shift),
            KeyCode::Right if ctrl => self.move_word_right(shift),
            KeyCode::Right => self.move_right(shift),
            KeyCode::Home => self.move_to(0, shift),
            KeyCode::End => self.move_to(self.len(), shift),
            KeyCode::Char(c) if ctrl => match c.to_ascii_lowercase() {
                'c' => self.copy_to_clipboard(),
                'x' => self.cut_to_clipboard(),
                'v' => self.paste_from_clipboard(numeric),
                'a' => self.select_all(),
                _ => return false,
            },
            KeyCode::Char(c) if !alt && !c.is_control() && (!numeric || c.is_ascii_digit()) => {
                self.insert_char(c);
            }
            _ => return false,
        }
        true
    }

    /// Copies the selection, or the whole field when nothing is selected.
    pub fn copy_to_clipboard(&self) {
        let text = self
            .selected_text()
            .unwrap_or_else(|| self.text().to_string());
        let _ = clipboard_utils::set_clipboard_text(&text);
    }

    /// Cuts the selection to the clipboard. Does nothing without a selection.
    pub fn cut_to_clipboard(&mut self) {
        if let Some(text) = self.selected_text() {
            let _ = clipboard_utils::set_clipboard_text(&text);
            self.delete_selection();
        }
    }

    /// Pastes the system clipboard at the caret, replacing the selection.
    pub fn paste_from_clipboard(&mut self, numeric: bool) {
        if let Some(content) = clipboard_utils::get_clipboard_content() {
            self.paste(&content, numeric);
        }
    }

    /// Inserts `content` at the caret (or over the selection), sanitising line
    /// breaks and, when `numeric`, non-digits. Returns whether anything changed.
    pub fn paste(&mut self, content: &str, numeric: bool) -> bool {
        let filtered: String = content
            .chars()
            .filter(|c| *c != '\n' && *c != '\r' && (!numeric || c.is_ascii_digit()))
            .collect();
        if filtered.is_empty() {
            return false;
        }
        self.insert_str(&filtered);
        true
    }

    /// Inserts a character at the caret, replacing the selection.
    pub fn insert_char(&mut self, c: char) {
        self.insert_str(&c.to_string());
    }

    /// Inserts a string at the caret, replacing the selection.
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.delete_selection();
        let idx = self.byte_index(self.cursor);
        self.text.insert_str(idx, s);
        self.cursor += s.chars().count();
        self.anchor = None;
    }

    /// Deletes the character before the caret (or the selection).
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        self.delete_range(self.cursor - 1, self.cursor)
    }

    /// Deletes the character after the caret (or the selection).
    pub fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor >= self.len() {
            return false;
        }
        self.delete_range(self.cursor, self.cursor + 1)
    }

    /// Deletes from the previous word boundary to the caret (or the selection).
    pub fn delete_word_back(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let chars: Vec<char> = self.text.chars().collect();
        self.delete_range(prev_word_boundary(&chars, self.cursor), self.cursor)
    }

    /// Deletes from the caret to the next word boundary (or the selection).
    pub fn delete_word_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor >= self.len() {
            return false;
        }
        let chars: Vec<char> = self.text.chars().collect();
        self.delete_range(self.cursor, next_word_boundary(&chars, self.cursor))
    }

    /// Deletes the selection, returning whether there was one.
    pub fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            return false;
        };
        let start = range.start;
        self.delete_range(range.start, range.end);
        self.cursor = start;
        true
    }

    /// Byte offset of char index `idx` (or the end of the text).
    fn byte_index(&self, idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(idx)
            .map_or(self.text.len(), |(i, _)| i)
    }

    fn delete_range(&mut self, start: usize, end: usize) -> bool {
        if start >= end || end > self.len() {
            return false;
        }
        let byte_start = self.byte_index(start);
        let byte_end = self.byte_index(end);
        self.text.replace_range(byte_start..byte_end, "");
        self.cursor = start;
        self.anchor = None;
        true
    }

    fn move_to(&mut self, pos: usize, extend: bool) {
        let pos = pos.min(self.len());
        if extend && self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        } else if !extend {
            self.anchor = None;
        }
        self.cursor = pos;
    }

    fn move_left(&mut self, extend: bool) {
        if !extend && let Some(sel) = self.selection() {
            self.move_to(sel.start, false);
            return;
        }
        self.move_to(self.cursor.saturating_sub(1), extend);
    }

    fn move_right(&mut self, extend: bool) {
        if !extend && let Some(sel) = self.selection() {
            self.move_to(sel.end, false);
            return;
        }
        self.move_to(self.cursor + 1, extend);
    }

    fn move_word_left(&mut self, extend: bool) {
        if !extend && let Some(sel) = self.selection() {
            self.move_to(sel.start, false);
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        self.move_to(prev_word_boundary(&chars, self.cursor), extend);
    }

    fn move_word_right(&mut self, extend: bool) {
        if !extend && let Some(sel) = self.selection() {
            self.move_to(sel.end, false);
            return;
        }
        let chars: Vec<char> = self.text.chars().collect();
        self.move_to(next_word_boundary(&chars, self.cursor), extend);
    }
}

/// A single-line editor for secret text, such as a password.
///
/// It wraps a [`TextEditState`] but does not expose the plaintext as `&str`;
/// the only way to read the value is [`SecretEdit::take_secret`], which moves
/// it into a [`SecretString`]. Clipboard copy and cut are disabled so a secret
/// cannot be exfiltrated through the system clipboard, and [`SecretEdit::clear`]
/// scrubs the buffer instead of merely shortening it.
#[derive(Default)]
pub struct SecretEdit {
    state: TextEditState,
}

impl SecretEdit {
    /// Creates an empty field.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the field is empty.
    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// The caret position (char index).
    #[cfg_attr(not(test), allow(dead_code))]
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.state.cursor()
    }

    /// Replaces the contents, moving the caret to the end and clearing the
    /// selection.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.state.set_text(text);
    }

    /// Empties the field, zeroing the previous contents.
    pub fn clear(&mut self) {
        self.state.text.zeroize();
        self.state.cursor = 0;
        self.state.anchor = None;
    }

    /// Clears the selection, keeping the caret where it is.
    pub fn clear_selection(&mut self) {
        self.state.clear_selection();
    }

    /// Moves the caret to `pos`, clearing any selection.
    pub fn set_cursor(&mut self, pos: usize) {
        self.state.set_cursor(pos);
    }

    /// Extends the selection to `pos` (anchoring at the caret when starting).
    pub fn extend_selection(&mut self, pos: usize) {
        self.state.extend_selection(pos);
    }

    /// Maps a click `column` to a caret position, taking scrolling into account.
    #[must_use]
    pub fn cursor_from_column(&self, column: usize, width: usize) -> usize {
        self.state.cursor_from_column(column, width)
    }

    /// Selects the word at `column` (used by double-click).
    pub fn select_word_at(&mut self, column: usize, width: usize) {
        self.state.select_word_at(column, width);
    }

    /// Handles one key press, returning whether it was consumed.
    ///
    /// Clipboard copy (`Ctrl+C`) and cut (`Ctrl+X`) are swallowed so the secret
    /// never reaches the system clipboard; paste and select-all still work.
    #[must_use]
    pub fn handle_key(&mut self, code: KeyCode, modifiers: Modifiers, numeric: bool) -> bool {
        if modifiers.contains(Modifiers::CONTROL)
            && matches!(code, KeyCode::Char('c' | 'C' | 'x' | 'X'))
        {
            return true;
        }
        self.state.handle_key(code, modifiers, numeric)
    }

    /// Takes the contents as a [`SecretString`], leaving the field empty.
    ///
    /// The editor's buffer is zeroed, so no plaintext copy is left behind.
    #[must_use]
    pub fn take_secret(&mut self) -> SecretString {
        let text = std::mem::take(&mut *self.state.text);
        self.state.text.zeroize();
        self.state.cursor = 0;
        self.state.anchor = None;
        SecretString::from(text)
    }

    /// A widget rendering this field, masked so the secret is never drawn in
    /// the clear. The caller may restyle it, but there is no unmasked default.
    #[must_use]
    pub fn widget<'a>(&'a self, placeholder: &'a str, focused: bool) -> TextEditWidget<'a> {
        TextEditWidget::new(&self.state, placeholder, focused).mask(Some('•'))
    }
}

/// A ratatui widget rendering a [`TextEditState`].
///
/// Rendering only draws text into the buffer; use [`render_text_edit`] to also
/// place the terminal cursor (a [`Widget`] cannot reach the frame).
pub struct TextEditWidget<'a> {
    state: &'a TextEditState,
    placeholder: &'a str,
    focused: bool,
    mask: Option<char>,
    text_color: Color,
    placeholder_color: Color,
    selection_style: Style,
    bg: Color,
}

impl<'a> TextEditWidget<'a> {
    /// Creates a widget for `state`, showing `placeholder` when empty.
    #[must_use]
    pub fn new(state: &'a TextEditState, placeholder: &'a str, focused: bool) -> Self {
        Self {
            state,
            placeholder,
            focused,
            mask: None,
            text_color: Color::Reset,
            placeholder_color: Color::DarkGray,
            selection_style: Style::default().bg(Color::DarkGray),
            bg: Color::Reset,
        }
    }

    /// Masks every character with `mask` (for password fields).
    #[must_use]
    pub fn mask(mut self, mask: Option<char>) -> Self {
        self.mask = mask;
        self
    }

    /// Sets the text colour.
    #[must_use]
    pub fn text_color(mut self, color: Color) -> Self {
        self.text_color = color;
        self
    }

    /// Sets the placeholder colour.
    #[must_use]
    pub fn placeholder_color(mut self, color: Color) -> Self {
        self.placeholder_color = color;
        self
    }

    /// Sets the style applied to selected characters.
    #[must_use]
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = style;
        self
    }

    /// Sets the field background colour.
    #[must_use]
    pub fn bg(mut self, color: Color) -> Self {
        self.bg = color;
        self
    }

    /// Whether the field is focused (the terminal cursor is only placed then).
    #[must_use]
    pub fn focused(&self) -> bool {
        self.focused
    }

    /// Screen position of the caret within `area`, if it is visible.
    #[must_use]
    pub fn cursor_screen_position(&self, area: Rect) -> Option<Position> {
        let width = area.width as usize;
        if width == 0 {
            return None;
        }
        let col = self
            .state
            .cursor()
            .saturating_sub(scroll_offset(self.state.cursor(), width));
        if col < width {
            Some(Position::new(
                area.x + u16::try_from(col).unwrap_or(u16::MAX),
                area.y,
            ))
        } else {
            None
        }
    }
}

impl Widget for TextEditWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let width = area.width as usize;

        if self.state.is_empty() && !self.placeholder.is_empty() {
            let text: String = self.placeholder.chars().take(width).collect();
            Paragraph::new(Line::from(Span::styled(
                text,
                Style::default().fg(self.placeholder_color).bg(self.bg),
            )))
            .style(Style::default().bg(self.bg))
            .render(area, buf);
            return;
        }

        let scroll = scroll_offset(self.state.cursor(), width);
        let selection = self.state.selection();
        let mut spans = Vec::new();
        for (idx, ch) in self
            .state
            .text()
            .chars()
            .enumerate()
            .skip(scroll)
            .take(width)
        {
            let display = self.mask.unwrap_or(ch);
            let mut style = Style::default().fg(self.text_color).bg(self.bg);
            if let Some(range) = selection.as_ref()
                && range.contains(&idx)
            {
                style = style.patch(self.selection_style);
            }
            spans.push(Span::styled(display.to_string(), style));
        }
        Paragraph::new(Line::from(spans))
            .style(Style::default().bg(self.bg))
            .render(area, buf);
    }
}

/// Renders an edit field and, when focused, places the terminal cursor.
pub fn render_text_edit(f: &mut ratatui::Frame, widget: TextEditWidget<'_>, area: Rect) {
    let cursor = widget
        .cursor_screen_position(area)
        .filter(|_| widget.focused());
    f.render_widget(widget, area);
    if let Some(pos) = cursor {
        f.set_cursor_position(pos);
    }
}

#[cfg(test)]
mod tests;
