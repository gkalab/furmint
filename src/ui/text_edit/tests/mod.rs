use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};

fn press(st: &mut TextEditState, code: KeyCode, modifiers: Modifiers) {
    let _ = st.handle_key(code, modifiers, false);
}

fn render_to_buffer(
    state: &TextEditState,
    width: u16,
    focused: bool,
    mask: Option<char>,
) -> Buffer {
    let area = Rect::new(0, 0, width, 1);
    let mut buf = Buffer::empty(area);
    let widget = TextEditWidget::new(state, "placeholder", focused).mask(mask);
    widget.render(area, &mut buf);
    buf
}

fn line(buf: &Buffer, len: u16) -> String {
    (0..len).map(|x| buf[(x, 0)].symbol().to_string()).collect()
}

#[test]
fn typing_inserts_and_moves_cursor() {
    let mut st = TextEditState::new();
    for c in "hello".chars() {
        press(&mut st, KeyCode::Char(c), Modifiers::NONE);
    }
    assert_eq!(st.text(), "hello");
    assert_eq!(st.cursor(), 5);

    press(&mut st, KeyCode::Left, Modifiers::NONE);
    press(&mut st, KeyCode::Left, Modifiers::NONE);
    assert_eq!(st.cursor(), 3);

    press(&mut st, KeyCode::Char('X'), Modifiers::NONE);
    assert_eq!(st.text(), "helXlo");
    assert_eq!(st.cursor(), 4);

    press(&mut st, KeyCode::Backspace, Modifiers::NONE);
    assert_eq!(st.text(), "hello");
    assert_eq!(st.cursor(), 3);
}

#[test]
fn shift_arrows_select_and_typing_replaces() {
    let mut st = TextEditState::with_text("abcdef");
    st.set_cursor(2);
    press(&mut st, KeyCode::Right, Modifiers::SHIFT);
    press(&mut st, KeyCode::Right, Modifiers::SHIFT);
    assert_eq!(st.selection(), Some(2..4));
    assert_eq!(st.selected_text().as_deref(), Some("cd"));

    press(&mut st, KeyCode::Char('Z'), Modifiers::NONE);
    assert_eq!(st.text(), "abZef");
    assert_eq!(st.cursor(), 3);
    assert!(st.selection().is_none());
}

#[test]
fn shift_home_selects_to_start() {
    let mut st = TextEditState::with_text("hello world");
    st.set_cursor(11);
    press(&mut st, KeyCode::Home, Modifiers::SHIFT);
    assert_eq!(st.selection(), Some(0..11));

    // An unshifted move collapses the selection to its edge.
    press(&mut st, KeyCode::Left, Modifiers::NONE);
    assert_eq!(st.cursor(), 0);
    assert!(st.selection().is_none());
}

#[test]
fn clear_selection_keeps_the_cursor() {
    let mut st = TextEditState::with_text("abcdef");
    st.set_cursor(2);
    st.extend_selection(5);
    assert!(st.selection().is_some());

    st.clear_selection();
    assert!(st.selection().is_none());
    assert_eq!(st.cursor(), 5);
}

#[test]
fn select_all_then_backspace_clears() {
    let mut st = TextEditState::with_text("hello");
    press(&mut st, KeyCode::Char('a'), Modifiers::CONTROL);
    assert_eq!(st.selection(), Some(0..5));

    press(&mut st, KeyCode::Backspace, Modifiers::NONE);
    assert_eq!(st.text(), "");
    assert_eq!(st.cursor(), 0);
}

#[test]
fn ctrl_arrows_move_by_word() {
    let mut st = TextEditState::with_text("foo bar baz");
    st.set_cursor(0);
    press(&mut st, KeyCode::Right, Modifiers::CONTROL);
    assert_eq!(st.cursor(), 3);
    press(&mut st, KeyCode::Right, Modifiers::CONTROL);
    assert_eq!(st.cursor(), 7);
    press(&mut st, KeyCode::Right, Modifiers::CONTROL);
    assert_eq!(st.cursor(), 11);

    press(&mut st, KeyCode::Left, Modifiers::CONTROL);
    assert_eq!(st.cursor(), 8);
    press(&mut st, KeyCode::Left, Modifiers::CONTROL);
    assert_eq!(st.cursor(), 4);
}

#[test]
fn ctrl_backspace_and_delete_remove_words() {
    let mut st = TextEditState::with_text("foo bar baz");
    st.set_cursor(0);
    press(&mut st, KeyCode::Delete, Modifiers::CONTROL);
    assert_eq!(st.text(), " bar baz");

    let mut st = TextEditState::with_text("foo bar baz");
    st.set_cursor(8);
    press(&mut st, KeyCode::Backspace, Modifiers::CONTROL);
    assert_eq!(st.text(), "foo baz");
    assert_eq!(st.cursor(), 4);
}

#[test]
fn underscrore_is_a_word_char() {
    let mut st = TextEditState::with_text("bar_baz");
    st.select_word_at(0, 20);
    assert_eq!(st.selected_text().as_deref(), Some("bar_baz"));
}

#[test]
fn paste_replaces_selection_and_strips_newlines() {
    let mut st = TextEditState::with_text("abc");
    st.select_all();
    assert!(st.paste("x\ny", false));
    assert_eq!(st.text(), "xy");
    assert_eq!(st.cursor(), 2);
}

#[test]
fn paste_respects_numeric_fields() {
    let mut st = TextEditState::with_text("12");
    st.set_cursor(2);
    assert!(st.paste("a3b4", true));
    assert_eq!(st.text(), "1234");

    let mut st = TextEditState::new();
    assert!(!st.paste("abc", true));
    assert!(!st.paste("\n\r", false));
}

#[test]
fn numeric_fields_reject_letters() {
    let mut st = TextEditState::new();
    let _ = st.handle_key(KeyCode::Char('a'), Modifiers::NONE, true);
    let _ = st.handle_key(KeyCode::Char('1'), Modifiers::NONE, true);
    assert_eq!(st.text(), "1");
}

#[test]
fn cursor_from_column_accounts_for_scroll() {
    let mut st = TextEditState::with_text("0123456789");
    st.set_cursor(10);
    // scroll = 10 - 4 + 1 = 7, so column 0 maps to char 7.
    assert_eq!(st.cursor_from_column(0, 4), 7);
    assert_eq!(st.cursor_from_column(3, 4), 10);
    // Beyond the end clamps to the text length.
    assert_eq!(st.cursor_from_column(99, 4), 10);
}

#[test]
fn double_click_selects_word() {
    let mut st = TextEditState::with_text("foo bar_baz");
    st.select_word_at(5, 20);
    assert_eq!(st.selected_text().as_deref(), Some("bar_baz"));
}

#[test]
fn widget_renders_text_and_scrolls_to_cursor() {
    let mut st = TextEditState::with_text("hello world");
    st.set_cursor(6);
    let buf = render_to_buffer(&st, 5, true, None);
    // scroll = 6 - 5 + 1 = 2 -> "llo w".
    assert_eq!(line(&buf, 5), "llo w");
}

#[test]
fn widget_scrolls_to_keep_cursor_visible_at_end() {
    let mut st = TextEditState::with_text("hello world");
    st.set_cursor(11);
    let buf = render_to_buffer(&st, 5, true, None);
    // Cursor sits in the last column, so the final cell is left blank.
    assert_eq!(line(&buf, 5), "orld ");
}

#[test]
fn widget_renders_placeholder_when_empty() {
    let st = TextEditState::new();
    let buf = render_to_buffer(&st, 20, true, None);
    assert_eq!(line(&buf, 11), "placeholder");
}

#[test]
fn widget_masks_each_character() {
    let st = TextEditState::with_text("secret");
    let buf = render_to_buffer(&st, 10, true, Some('•'));
    assert_eq!(line(&buf, 6), "••••••");
}

#[test]
fn widget_highlights_selection() {
    let mut st = TextEditState::with_text("abcdef");
    st.set_cursor(1);
    st.extend_selection(4);
    let buf = render_to_buffer(&st, 10, true, None);
    // Selected cells carry the selection background.
    assert_eq!(buf[(1, 0)].bg, Color::DarkGray);
    assert_eq!(buf[(3, 0)].bg, Color::DarkGray);
    assert_ne!(buf[(0, 0)].bg, Color::DarkGray);
    assert_ne!(buf[(4, 0)].bg, Color::DarkGray);
}

#[test]
fn cursor_screen_position_respects_scroll() {
    let mut st = TextEditState::with_text("hello world");
    st.set_cursor(11);
    let area = Rect::new(3, 2, 5, 1);
    let widget = TextEditWidget::new(&st, "", true);
    // scroll 7, column 4 -> x = 3 + 4 = 7.
    assert_eq!(
        widget.cursor_screen_position(area),
        Some(Position::new(7, 2))
    );
}

#[test]
fn secret_edit_take_secret_returns_value_and_empties_field() {
    use secrecy::ExposeSecret;

    let mut ed = SecretEdit::new();
    for c in "s3cret".chars() {
        let _ = ed.handle_key(KeyCode::Char(c), Modifiers::NONE, false);
    }

    let secret = ed.take_secret();
    assert_eq!(secret.expose_secret(), "s3cret");
    assert!(ed.is_empty());
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn secret_edit_ignores_copy_and_cut() {
    use secrecy::ExposeSecret;

    let mut ed = SecretEdit::new();
    ed.set_text("s3cret");
    // Select everything, then try to copy and cut it.
    let _ = ed.handle_key(KeyCode::Char('a'), Modifiers::CONTROL, false);
    assert!(ed.handle_key(KeyCode::Char('c'), Modifiers::CONTROL, false));
    assert!(ed.handle_key(KeyCode::Char('x'), Modifiers::CONTROL, false));
    // Cut would have deleted the selection; the secret must be untouched.
    assert_eq!(ed.cursor(), 6);
    assert_eq!(ed.take_secret().expose_secret(), "s3cret");
}

#[test]
fn secret_edit_clear_empties_field() {
    let mut ed = SecretEdit::new();
    ed.set_text("s3cret");
    ed.clear();
    assert!(ed.is_empty());
    assert_eq!(ed.cursor(), 0);
}

#[test]
fn secret_edit_widget_masks_text() {
    let mut ed = SecretEdit::new();
    ed.set_text("secret");
    let area = Rect::new(0, 0, 10, 1);
    let mut buf = Buffer::empty(area);
    ed.widget("placeholder", true).render(area, &mut buf);
    assert_eq!(line(&buf, 6), "••••••");
}
