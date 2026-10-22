use crate::app::AppState;
use crate::tasks::UiEvent;
use tokio::sync::mpsc;

mod connect;
mod fields;
mod history;
mod parse;

async fn basic_app_state() -> AppState {
    let (task_tx, _task_rx) = mpsc::unbounded_channel::<UiEvent>();

    crate::test_utils::TestAppBuilder::new()
        .left(
            crate::app_state::tabs::TabManager::new(&std::env::temp_dir())
                .await
                .unwrap(),
        )
        .right(
            crate::app_state::tabs::TabManager::new(&std::env::temp_dir())
                .await
                .unwrap(),
        )
        .task_tx(task_tx)
        .build()
}
