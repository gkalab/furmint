use super::*;
use crate::app::AppState;
use crate::config::EditorConfig;
use crate::fs::utils::FileEntry;
use tokio::sync::mpsc::unbounded_channel;

#[tokio::test]
async fn test_handle_edit_invalid_command_sets_error() {
    let (_tx, _) = unbounded_channel::<crate::tasks::UiEvent>();
    let entry = FileEntry {
        name: "file.txt".to_string(),
        is_dir: false,
        is_symlink: false,
        size: Some(10),
        modified: None,
        attributes: String::new(),
        selected: false,
    };
    let mut app = AppState::test_default();
    app.panels.left.tabs[0].entries = vec![entry];
    app.panels.left.tabs[0].current_dir = std::path::PathBuf::from("/tmp");
    app.panels.left.tabs[0].recompute_visible_indices();
    app.editor_cfg = EditorConfig {
        command: Some(String::new()),
        in_terminal: Some(true),
    };
    handle_edit(&mut app).await;
    let error = app.panels.left.active_tab().error.clone();
    assert!(
        error.is_some(),
        "Error should be set if invalid editor command"
    );
}

#[tokio::test]
async fn test_spawn_editor_detached_invalid_command() {
    let result = spawn_editor_detached(Some(""), std::path::Path::new("/tmp/test.txt"));
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid editor command")
    );
}

#[tokio::test]
async fn test_resolve_editor_default() {
    let (program, args) = resolve_editor(None, std::path::Path::new("/tmp/test.txt")).unwrap();
    assert_ne!(program, "");
    assert_eq!(args, vec!["/tmp/test.txt".to_string()]);
}

#[tokio::test]
async fn test_spawn_editor_detached_valid_command() {
    let result = spawn_editor_detached(Some("true"), std::path::Path::new("/tmp/test.txt"));
    assert!(result.is_ok());
}
