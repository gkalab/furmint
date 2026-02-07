// Shared input handling utilities

use crate::handlers::clipboard_utils::{get_clipboard_content, insert_text_at_cursor_unicode};
use crossterm::event::{KeyCode, KeyModifiers};

pub fn handle_text_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    text: &mut String,
    cursor_position: &mut usize,
    is_numeric: bool,
) -> bool {
    match code {
        KeyCode::Backspace => {
            if *cursor_position > 0
                && let Some((byte_idx, _)) = text.char_indices().nth(*cursor_position - 1) {
                    text.remove(byte_idx);
                    *cursor_position -= 1;
                    return true;
                }
        }
        KeyCode::Delete => {
            let current_len = text.chars().count();
            if *cursor_position < current_len
                && let Some((byte_idx, _)) = text.char_indices().nth(*cursor_position) {
                    text.remove(byte_idx);
                    return true;
                }
        }
        KeyCode::Left => {
            if *cursor_position > 0 {
                *cursor_position -= 1;
            }
        }
        KeyCode::Right => {
            let len = text.chars().count();
            if *cursor_position < len {
                *cursor_position += 1;
            }
        }
        KeyCode::Home => {
            *cursor_position = 0;
        }
        KeyCode::End => {
            *cursor_position = text.chars().count();
        }
        KeyCode::Char('v') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(content) = get_clipboard_content() {
                let sanitized = if is_numeric {
                    content.chars().filter(|c| c.is_ascii_digit()).collect()
                } else {
                    content
                };
                if !sanitized.is_empty() {
                    insert_text_at_cursor_unicode(text, cursor_position, &sanitized);
                    return true;
                }
            }
        }
        KeyCode::Char(c) => {
            if !is_numeric || c.is_ascii_digit() {
                let idx = *cursor_position;
                let current_len = text.chars().count();
                if idx >= current_len {
                    text.push(c);
                } else if let Some((byte_idx, _)) = text.char_indices().nth(idx) {
                    text.insert(byte_idx, c);
                }
                *cursor_position += 1;
                return true;
            }
        }
        _ => {}
    }
    false
}
