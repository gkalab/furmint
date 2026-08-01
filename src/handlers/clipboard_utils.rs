use std::sync::{Mutex, MutexGuard};

// arboard's X11 backend serves clipboard data from a hidden window that must stay alive for the
// data to remain available, so the clipboard instance is kept for the lifetime of the process
// instead of being created and dropped on each operation.
static SYSTEM_CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

/// Returns the process-wide system text clipboard, initializing it on first use.
///
/// # Panics
///
/// Panics if the clipboard mutex is poisoned.
fn system_clipboard() -> Result<MutexGuard<'static, Option<arboard::Clipboard>>, arboard::Error> {
    let mut guard = SYSTEM_CLIPBOARD.lock().unwrap();
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new()?);
    }
    Ok(guard)
}

#[must_use]
pub fn get_clipboard_content() -> Option<String> {
    system_clipboard().ok()?.as_mut()?.get_text().ok()
}

/// Sets the system clipboard to the given text.
///
/// # Errors
///
/// Returns an error if the clipboard cannot be accessed or updated.
///
/// # Panics
///
/// Panics if the clipboard mutex is poisoned.
pub fn set_clipboard_text(text: &str) -> Result<(), arboard::Error> {
    system_clipboard()?
        .as_mut()
        .expect("clipboard was just initialized")
        .set_text(text)
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
