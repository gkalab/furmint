use fm::config::KeyboardConfig;
use std::path::PathBuf;
use termina::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, Modifiers};

fn key_event(code: KeyCode) -> Event {
    Event::Key(KeyEvent {
        code,
        modifiers: Modifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    })
}

fn open_fuzzy_search(app: &mut fm::AppState, keyboard: &KeyboardConfig, dirs: &[&str]) {
    for dir in dirs {
        app.dir_history.record_visit("local", &PathBuf::from(dir));
    }
    app.fuzzy_search.list.is_visible = true;
    app.fuzzy_search.reset();
    app.fuzzy_search.list.is_visible = true;
    let context_key = app.active_tab().provider.context_key();
    let results = app.dir_history.fuzzy_search(&context_key.to_string(), "");
    app.fuzzy_search.list.items = results
        .into_iter()
        .map(|(p, _)| fm::ui::filterable_list::ListItem::from_path(p))
        .collect();
    app.fuzzy_search.list.selected_index = 0;
    let _ = keyboard;
}

#[tokio::test]
async fn delete_removes_selected_entry_without_confirmation() {
    let keyboard = KeyboardConfig::default();
    let mut app = fm::AppState::test_default();
    app.dir_history.entries.clear();
    open_fuzzy_search(&mut app, &keyboard, &["/tmp/alpha", "/tmp/beta"]);

    assert_eq!(app.fuzzy_search.list.items.len(), 2);
    let target = app.fuzzy_search.get_selected_dir().expect("selected entry");

    fm::handlers::main_handler::route_event(key_event(KeyCode::Delete), &mut app, &keyboard).await;

    assert!(
        !app.dir_history
            .entries
            .get("local")
            .is_some_and(|entries| entries.contains_key(&target)),
        "entry should be gone from the history"
    );
    assert_eq!(app.fuzzy_search.list.items.len(), 1);
    assert!(app.fuzzy_search.list.is_visible, "popup stays open");
    assert!(!app.popups.any_visible(), "no confirmation popup is shown");
}

#[tokio::test]
async fn delete_removes_entry_under_active_filter() {
    let keyboard = KeyboardConfig::default();
    let mut app = fm::AppState::test_default();
    app.dir_history.entries.clear();
    open_fuzzy_search(&mut app, &keyboard, &["/tmp/alpha", "/tmp/beta"]);

    // Filter down to a single result, then delete it.
    route_char(&mut app, &keyboard, 'b').await;
    assert_eq!(app.fuzzy_search.list.items.len(), 1);
    let target = app.fuzzy_search.get_selected_dir().expect("selected entry");

    fm::handlers::main_handler::route_event(key_event(KeyCode::Delete), &mut app, &keyboard).await;

    assert!(!app.dir_history.entries["local"].contains_key(&target));
    assert!(app.fuzzy_search.list.items.is_empty());
}

#[tokio::test]
async fn repeated_delete_stays_on_the_same_row_until_the_end() {
    let keyboard = KeyboardConfig::default();
    let mut app = fm::AppState::test_default();
    app.dir_history.entries.clear();
    open_fuzzy_search(&mut app, &keyboard, &["/tmp/a1", "/tmp/b2", "/tmp/c3"]);

    assert_eq!(app.fuzzy_search.list.items.len(), 3);
    let first = app.fuzzy_search.get_selected_dir().expect("selected entry");

    // The removed item shifts up, so the cursor keeps its row and now points at
    // what was the next entry.
    route_event_delete(&mut app, &keyboard).await;
    assert_eq!(app.fuzzy_search.list.selected_index, 0);
    let second = app.fuzzy_search.get_selected_dir().expect("selected entry");
    assert_ne!(first, second);
    assert_eq!(app.fuzzy_search.list.items.len(), 2);

    // Last row: the index clamps once the shorter list is exhausted.
    route_event_delete(&mut app, &keyboard).await;
    assert_eq!(app.fuzzy_search.list.selected_index, 0);
    assert_eq!(app.fuzzy_search.list.items.len(), 1);

    route_event_delete(&mut app, &keyboard).await;
    assert!(app.fuzzy_search.list.items.is_empty());
    assert_eq!(app.fuzzy_search.list.selected_index, 0);
}

#[tokio::test]
async fn delete_keeps_selection_on_the_row_after_a_middle_removal() {
    let keyboard = KeyboardConfig::default();
    let mut app = fm::AppState::test_default();
    app.dir_history.entries.clear();
    open_fuzzy_search(&mut app, &keyboard, &["/tmp/a1", "/tmp/b2", "/tmp/c3"]);

    app.fuzzy_search.list.selected_index = 1;
    let target = app.fuzzy_search.get_selected_dir().expect("selected entry");

    route_event_delete(&mut app, &keyboard).await;

    assert_eq!(app.fuzzy_search.list.selected_index, 1);
    assert!(app.fuzzy_search.list.items.iter().all(|i| i.path != target));
}

#[tokio::test]
async fn delete_on_empty_list_is_a_no_op() {
    let keyboard = KeyboardConfig::default();
    let mut app = fm::AppState::test_default();
    app.dir_history.entries.clear();
    open_fuzzy_search(&mut app, &keyboard, &[]);
    assert!(app.fuzzy_search.list.items.is_empty());

    route_event_delete(&mut app, &keyboard).await;

    assert!(app.fuzzy_search.list.is_visible);
    assert!(app.active_tab().error.is_none());
}

async fn route_char(app: &mut fm::AppState, keyboard: &KeyboardConfig, c: char) {
    fm::handlers::main_handler::route_event(key_event(KeyCode::Char(c)), app, keyboard).await;
}

async fn route_event_delete(app: &mut fm::AppState, keyboard: &KeyboardConfig) {
    fm::handlers::main_handler::route_event(key_event(KeyCode::Delete), app, keyboard).await;
}
