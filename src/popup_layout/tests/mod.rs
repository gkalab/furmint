use super::*;
use crate::app::AppState;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const W: u16 = 80;
const H: u16 = 24;

fn screen() -> Rect {
    Rect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    }
}

fn app_with(setup: impl FnOnce(&mut AppState)) -> AppState {
    let mut app = AppState::test_default();
    setup(&mut app);
    app
}

/// Draws every popup at the test size and returns the rendered screen text.
///
/// `event_loop::draw_ui` assigns `app.layout.popups` before drawing, so the
/// renderers receive the frame's geometry. The test must do the same or the
/// popups draw with empty geometry.
fn draw_popups(app: &mut AppState, layout: &PopupLayout) -> String {
    let palette = crate::theme::default_theme();
    let keyboard = crate::config::KeyboardConfig::default();
    let backend = TestBackend::new(W, H);
    let mut terminal = Terminal::new(backend).unwrap();
    app.layout.popups = layout.clone();
    terminal
        .draw(|f| {
            crate::ui::main_ui::draw_all_popups(f, app, &palette, &keyboard);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..H)
        .map(|y| (0..W).map(|x| buffer[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every button rect must be non-degenerate and fully inside the screen,
/// otherwise hit-testing can never match what the user sees.
fn assert_usable(label: &str, buttons: &[Rect]) {
    assert!(!buttons.is_empty(), "{label} produced no button rects");
    for rect in buttons {
        assert!(
            rect.width > 0 && rect.height > 0,
            "{label} degenerate {rect:?}"
        );
        assert!(
            rect.right() <= W && rect.bottom() <= H,
            "{label} button {rect:?} escapes the {W}x{H} screen"
        );
    }
}

/// The renderers draw buttons at exactly these rects, so a rect containing
/// its label proves hit-testing and rendering agree.
#[test]
fn rendered_buttons_land_inside_their_computed_rects() {
    type PopupCase = (
        &'static str,
        Box<dyn Fn(&mut AppState)>,
        crate::app::PopupKind,
        &'static [&'static str],
    );

    let cases: Vec<PopupCase> = vec![
        (
            "error",
            Box::new(|a: &mut AppState| a.popups.error.is_visible = true),
            crate::app::PopupKind::Error,
            &["Cancel", "Skip", "Skip All", "Retry"],
        ),
        // Two rows: three buttons then two. Both trailing buttons read "All".
        (
            "conflict",
            Box::new(|a: &mut AppState| a.popups.conflict.is_visible = true),
            crate::app::PopupKind::Conflict,
            &["Cancel", "Skip", "Overwrite", "All", "All"],
        ),
        (
            "rename",
            Box::new(|a: &mut AppState| {
                a.popups.rename.is_visible = true;
                a.popups.rename.show_overwrite_confirm = true;
            }),
            crate::app::PopupKind::Rename,
            &["No", "Yes"],
        ),
        (
            "remote_edit",
            Box::new(|a: &mut AppState| a.popups.remote_edit.is_visible = true),
            crate::app::PopupKind::RemoteEdit,
            &["Cancel", "Upload"],
        ),
        (
            "host_key",
            Box::new(|a: &mut AppState| a.popups.host_key.is_visible = true),
            crate::app::PopupKind::HostKey,
            &["Reject", "Accept"],
        ),
        (
            "delete",
            Box::new(|a: &mut AppState| a.popups.delete.is_visible = true),
            crate::app::PopupKind::Delete,
            &["No", "Yes"],
        ),
        (
            "empty_trash",
            Box::new(|a: &mut AppState| a.popups.empty_trash.is_visible = true),
            crate::app::PopupKind::EmptyTrash,
            &["No", "Yes"],
        ),
        (
            "quit",
            Box::new(|a: &mut AppState| a.popups.quit_confirmation.is_visible = true),
            crate::app::PopupKind::QuitConfirmation,
            &["No", "Yes"],
        ),
    ];

    for (name, show, kind, labels) in cases {
        let mut app = app_with(|a| show(a));
        let layout = compute_popup_layout(&app, screen());
        let buttons = layout.button_areas(kind).to_vec();
        assert_usable(name, &buttons);
        assert_eq!(
            buttons.len(),
            labels.len(),
            "{name} button count changed; update this test"
        );

        let rendered = draw_popups(&mut app, &layout);
        for (rect, label) in buttons.iter().zip(labels) {
            // Buttons are multi-row; their label sits somewhere inside the rect.
            let slice: String = (rect.y..rect.bottom())
                .map(|y| {
                    rendered
                        .lines()
                        .nth(usize::from(y))
                        .unwrap_or_default()
                        .chars()
                        .skip(usize::from(rect.x))
                        .take(usize::from(rect.width))
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                slice.contains(label),
                "{name}: label {label:?} is not drawn inside its button rect \
                 {rect:?} (row slices {slice:?})"
            );
        }
    }
}

#[test]
fn confirmation_overlays_get_their_own_geometry() {
    let app = app_with(|a| {
        a.popups.ssh_connection.is_visible = true;
        a.popups.ssh_connection.confirmation = Some(crate::state::ConfirmationState::new(
            "Delete?".to_string(),
            false,
            crate::state::ConfirmationAction::None,
        ));
        a.popups.bookmark.list.is_visible = true;
        a.popups.bookmark.confirmation = Some(crate::state::ConfirmationState::new(
            "Delete?".to_string(),
            false,
            crate::state::ConfirmationAction::None,
        ));
    });
    let layout = compute_popup_layout(&app, screen());
    assert_usable(
        "ssh confirmation",
        &layout.ssh_connection.confirmation_buttons.button_areas,
    );
    assert_usable(
        "bookmark confirmation",
        &layout.bookmark.confirmation_buttons.button_areas,
    );
}

/// List and table areas must be non-degenerate and inside the screen while
/// their popup is open, and absent once it is closed.
#[test]
fn list_and_table_areas_follow_visibility() {
    let items = vec![
        crate::ui::filterable_list::ListItem::from_path("/tmp/one".into()),
        crate::ui::filterable_list::ListItem::from_path("/tmp/two".into()),
    ];
    let app = app_with(|a| {
        a.fuzzy_search.list.is_visible = true;
        a.fuzzy_search.list.items = items.clone();
        a.popups.bookmark.list.is_visible = true;
        a.popups.bookmark.list.items = items;
        a.popups.help.is_visible = true;
        a.popups.ssh_connection.is_visible = true;
    });
    let layout = compute_popup_layout(&app, screen());

    for (label, area) in [
        ("fuzzy search", layout.fuzzy_search.list_area),
        ("bookmark", layout.bookmark.list_area),
        ("help table", layout.help.list_area),
        ("ssh history", layout.ssh_connection.history_area),
    ] {
        let area = area.unwrap_or_else(|| panic!("{label} area missing"));
        assert!(
            area.width > 0 && area.height > 0,
            "{label} degenerate {area:?}"
        );
        assert!(
            area.right() <= W && area.bottom() <= H,
            "{label} area {area:?} escapes the {W}x{H} screen"
        );
    }

    assert_eq!(layout.ssh_connection.field_areas.len(), 4);
    for area in &layout.ssh_connection.field_areas {
        assert!(
            area.width > 0 && area.height > 0,
            "ssh field degenerate {area:?}"
        );
    }
}

/// Every popup must report empty geometry while hidden, so a stale area can
/// never be hit-tested.
#[test]
fn hidden_popups_get_empty_geometry() {
    let app = AppState::test_default();
    let layout = compute_popup_layout(&app, screen());

    assert_eq!(layout.error.button_areas, [] as [ratatui::prelude::Rect; 0]);
    assert_eq!(
        layout.conflict.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert_eq!(
        layout.rename.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert_eq!(
        layout.remote_edit.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert_eq!(
        layout.host_key.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert_eq!(
        layout.delete.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert_eq!(
        layout.empty_trash.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert_eq!(
        layout.quit_confirmation.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert_eq!(
        layout.ssh_connection.field_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert!(layout.ssh_connection.history_area.is_none());
    assert_eq!(
        layout.ssh_connection.confirmation_buttons.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert!(layout.bookmark.list_area.is_none());
    assert_eq!(
        layout.bookmark.confirmation_buttons.button_areas,
        [] as [ratatui::prelude::Rect; 0]
    );
    assert!(layout.fuzzy_search.list_area.is_none());
    assert!(layout.help.list_area.is_none());
}

/// Popups that are open but have no confirmation overlay must not borrow the
/// overlay's buttons.
#[test]
fn confirmation_geometry_is_gated_on_the_overlay_being_open() {
    let app = app_with(|a| a.popups.ssh_connection.is_visible = true);
    let layout = compute_popup_layout(&app, screen());
    assert!(
        layout
            .ssh_connection
            .confirmation_buttons
            .button_areas
            .is_empty(),
        "ssh popup has no confirmation open but reported button areas"
    );
}
