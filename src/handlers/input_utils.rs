// Shared input handling utilities

use crate::handlers::clipboard_utils::{get_clipboard_content, insert_text_at_cursor_unicode};
use crossterm::event::{KeyCode, KeyModifiers};

#[must_use]
pub fn keyevent_to_string(code: KeyCode, modifiers: KeyModifiers) -> String {
    let mut parts: Vec<String> = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift".to_string());
    }
    let key = match code {
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "BackTab".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => String::new(),
    };
    parts.push(key);
    parts.join("-")
}

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
                && let Some((byte_idx, _)) = text.char_indices().nth(*cursor_position - 1)
            {
                text.remove(byte_idx);
                *cursor_position -= 1;
                return true;
            }
        }
        KeyCode::Delete => {
            let current_len = text.chars().count();
            if *cursor_position < current_len
                && let Some((byte_idx, _)) = text.char_indices().nth(*cursor_position)
            {
                text.remove(byte_idx);
                return true;
            }
        }
        KeyCode::Left if *cursor_position > 0 => {
            *cursor_position -= 1;
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
                    content.chars().filter(char::is_ascii_digit).collect()
                } else {
                    content
                };
                if !sanitized.is_empty() {
                    insert_text_at_cursor_unicode(text, cursor_position, &sanitized);
                    return true;
                }
            }
        }
        KeyCode::Char(c) if (!is_numeric || c.is_ascii_digit()) => {
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
        _ => {}
    }
    false
}
