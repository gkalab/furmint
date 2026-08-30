use fm::app::AppState;
use fm::handlers::popup_conflict::handle_conflict_event;
use fm::tasks::TaskDecision;
use termina::event::KeyCode;
use tokio::sync::mpsc;

async fn app_with_conflict(task_id: usize) -> (AppState, mpsc::Receiver<TaskDecision>) {
    use fm::tasks::TaskEvent;
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<TaskEvent>();
    let (dec_tx, dec_rx) = mpsc::channel(1);
    let mut app = fm::test_utils::TestAppBuilder::new()
        .left(
            fm::app::TabManager::new(&std::env::temp_dir())
                .await
                .unwrap(),
        )
        .right(
            fm::app::TabManager::new(&std::env::temp_dir())
                .await
                .unwrap(),
        )
        .task_tx(task_tx)
        .build();
    app.task_decision_txs.insert(task_id, dec_tx);

    app.popups.conflict.is_visible = true;
    app.popups.conflict.task_id = task_id;
    (app, dec_rx)
}

#[tokio::test]
async fn ignores_irrelevant_keys() {
    let (mut app, mut rx) = app_with_conflict(42).await;
    let _ = handle_conflict_event(KeyCode::Char('z'), &mut app).await;
    assert!(
        app.popups.conflict.is_visible,
        "Unmapped key should not reset popup"
    );
    assert!(
        rx.try_recv().is_err(),
        "No decision should be sent for unmapped key"
    );
}
