#[must_use]
pub fn get_clipboard_content() -> Option<String> {
    use clipboard::ClipboardContext;
    use clipboard::ClipboardProvider;

    let mut ctx: ClipboardContext = ClipboardContext::new().ok()?;
    ctx.get_contents().ok()
}

pub fn insert_text_at_cursor_unicode(text: &mut String, cursor: &mut usize, content: &str) {
    let sanitized: String = content
        .chars()
        .filter(|c| *c != '\n' && *c != '\r')
        .collect();

    let byte_pos = text
        .char_indices()
        .nth(*cursor)
        .map_or(text.len(), |(i, _)| i);

    text.insert_str(byte_pos, &sanitized);
    *cursor += sanitized.chars().count();
}
