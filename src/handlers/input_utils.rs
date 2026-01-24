// Shared input handling utilities

use crate::handlers::clipboard_utils::{get_clipboard_content, insert_text_at_cursor_unicode};
use crossterm::event::{KeyCode, KeyModifiers};

pub fn handle_text_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    text: &mut String,
    cursor_position: &mut usize,
) {
    match code {
        KeyCode::Backspace => {
            if *cursor_position > 0 {
                let current_len = text.chars().count();
                if *cursor_position <= current_len {
                    // Remove char at cursor_position - 1
                    let byte_idx = text
                        .char_indices()
                        .nth(*cursor_position - 1)
                        .map(|(i, _)| i)
                        .unwrap();
                    text.remove(byte_idx);
                    *cursor_position -= 1;
                }
            }
        }
        KeyCode::Delete => {
            let current_len = text.chars().count();
            if *cursor_position < current_len {
                let byte_idx = text
                    .char_indices()
                    .nth(*cursor_position)
                    .map(|(i, _)| i)
                    .unwrap();
                text.remove(byte_idx);
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
                insert_text_at_cursor_unicode(text, cursor_position, &content);
            }
        }
        KeyCode::Char(c) => {
            let idx = *cursor_position;
            let current_len = text.chars().count();
            if idx >= current_len {
                text.push(c);
            } else {
                let byte_idx = text.char_indices().nth(idx).map(|(i, _)| i).unwrap();
                text.insert(byte_idx, c);
            }
            *cursor_position += 1;
        }
        _ => {}
    }
}
