//! Screen geometry for every popup, computed once per frame before drawing.
//!
//! Mirrors [`crate::layout::LayoutState`] for the main window: the render pass
//! consumes this geometry instead of recomputing it, and the mouse handlers
//! read it from [`crate::app::AppState::layout`]

use ratatui::layout::{Constraint, Layout, Margin, Rect};

use crate::app::AppState;
use crate::ui::ui_utils::{
    centered_rect_absolute, centered_rect_percent, compute_button_rects, confirmation_button_areas,
};

/// Screen areas of a popup's button row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ButtonGeometry {
    pub button_areas: Vec<Rect>,
}

/// Inner area of a popup's scrollable list or table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListGeometry {
    pub list_area: Option<Rect>,
}

/// Geometry of the SSH connection popup: its input fields, its history list,
/// and the confirmation overlay it can spawn.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SshGeometry {
    /// Input areas, in `SshField` declaration order: connection string, name,
    /// port, history.
    pub field_areas: Vec<Rect>,
    pub history_area: Option<Rect>,
    pub confirmation_buttons: ButtonGeometry,
}

/// Geometry of the bookmark popup: its filterable list and the confirmation
/// overlay it can spawn.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BookmarkGeometry {
    pub list_area: Option<Rect>,
    pub confirmation_buttons: ButtonGeometry,
}

/// Every popup's frame geometry for one frame.
///
/// Popups that are hidden get empty geometry, so a stale area can never be
/// hit-tested.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PopupLayout {
    pub error: ButtonGeometry,
    pub conflict: ButtonGeometry,
    pub rename: ButtonGeometry,
    pub remote_edit: ButtonGeometry,
    pub host_key: ButtonGeometry,
    pub delete: ButtonGeometry,
    pub empty_trash: ButtonGeometry,
    pub quit_confirmation: ButtonGeometry,
    pub ssh_connection: SshGeometry,
    pub bookmark: BookmarkGeometry,
    pub fuzzy_search: ListGeometry,
    pub help: ListGeometry,
}

impl PopupLayout {
    /// Button areas of a popup that renders a button row. Empty when the popup is
    /// hidden or draws no buttons.
    #[must_use]
    pub fn button_areas(&self, kind: crate::app::PopupKind) -> &[Rect] {
        use crate::app::PopupKind;
        match kind {
            PopupKind::Error => &self.error.button_areas,
            PopupKind::Conflict => &self.conflict.button_areas,
            PopupKind::Rename => &self.rename.button_areas,
            PopupKind::RemoteEdit => &self.remote_edit.button_areas,
            PopupKind::QuitConfirmation => &self.quit_confirmation.button_areas,
            PopupKind::Delete => &self.delete.button_areas,
            PopupKind::EmptyTrash => &self.empty_trash.button_areas,
            PopupKind::HostKey => &self.host_key.button_areas,
            PopupKind::SshConnection => &self.ssh_connection.confirmation_buttons.button_areas,
            PopupKind::Bookmark => &self.bookmark.confirmation_buttons.button_areas,
            _ => &[],
        }
    }
}

/// Computes the popup geometry for a terminal of the given size.
#[must_use]
pub fn compute_popup_layout(app: &AppState, screen: Rect) -> PopupLayout {
    let popups = &app.popups;
    PopupLayout {
        error: error_geometry(screen, popups.error.is_visible),
        conflict: conflict_geometry(screen, popups.conflict.is_visible),
        rename: rename_geometry(
            screen,
            popups.rename.is_visible,
            popups.rename.show_overwrite_confirm,
        ),
        remote_edit: remote_edit_geometry(screen, popups.remote_edit.is_visible),
        host_key: host_key_geometry(screen, popups.host_key.is_visible),
        delete: confirmation_geometry(66, 6, screen, popups.delete.is_visible),
        empty_trash: confirmation_geometry(60, 6, screen, popups.empty_trash.is_visible),
        quit_confirmation: confirmation_geometry(
            50,
            7,
            screen,
            popups.quit_confirmation.is_visible,
        ),
        ssh_connection: ssh_geometry(
            screen,
            popups.ssh_connection.is_visible,
            popups.ssh_connection.confirmation.is_some(),
        ),
        bookmark: bookmark_geometry(
            screen,
            popups.bookmark.list.is_visible,
            popups.bookmark.confirmation.is_some(),
        ),
        fuzzy_search: fuzzy_search_geometry(screen, app.fuzzy_search.list.is_visible),
        help: help_geometry(screen, popups.help.is_visible),
    }
}

const ERROR_WIDTH: u16 = 60;
const ERROR_HEIGHT: u16 = 11;

#[must_use]
pub fn error_geometry(screen: Rect, visible: bool) -> ButtonGeometry {
    if !visible {
        return ButtonGeometry::default();
    }
    let popup_area = centered_rect_absolute(ERROR_WIDTH, ERROR_HEIGHT, screen);
    let content_area = Rect {
        x: popup_area.x + 3,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(6),
        height: popup_area.height.saturating_sub(1),
    };
    let layout = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Length(1), // Spacing below Title
        Constraint::Length(1), // Path label
        Constraint::Length(1), // Path
        Constraint::Min(3),    // Error message
        Constraint::Length(3), // Button row
    ])
    .split(content_area);

    ButtonGeometry {
        button_areas: compute_button_rects(
            &["[C]ancel", "[S]kip", "Skip [A]ll", "[R]etry"],
            layout[5],
        ),
    }
}

const CONFLICT_WIDTH: u16 = 60;
const CONFLICT_HEIGHT: u16 = 12;

#[must_use]
pub fn conflict_geometry(screen: Rect, visible: bool) -> ButtonGeometry {
    if !visible {
        return ButtonGeometry::default();
    }
    let popup_area = centered_rect_absolute(CONFLICT_WIDTH, CONFLICT_HEIGHT, screen);
    let content_area = Rect {
        x: popup_area.x + 3,
        y: popup_area.y + 2,
        width: popup_area.width.saturating_sub(6),
        height: popup_area.height.saturating_sub(2),
    };
    let inner_layout = Layout::vertical([
        Constraint::Min(3),    // Message + Path
        Constraint::Length(1), // Spacing
        Constraint::Length(3), // Row 1 buttons
        Constraint::Length(3), // Row 2 buttons
    ])
    .split(content_area);

    let mut areas = compute_button_rects(&["[C]ancel", "[S]kip", "[O]verwrite"], inner_layout[2]);
    areas.extend(compute_button_rects(
        &["Ski[p] All", "Overwrite [A]ll"],
        inner_layout[3],
    ));
    ButtonGeometry {
        button_areas: areas,
    }
}

const RENAME_WIDTH: u16 = 60;
const RENAME_HEIGHT: u16 = 5;
const RENAME_CONFIRM_HEIGHT: u16 = 7;

#[must_use]
pub fn rename_geometry(
    screen: Rect,
    visible: bool,
    show_overwrite_confirm: bool,
) -> ButtonGeometry {
    // Only the overwrite-confirm variant draws buttons.
    if !visible || !show_overwrite_confirm {
        return ButtonGeometry::default();
    }
    let popup_area = centered_rect_absolute(RENAME_WIDTH, RENAME_CONFIRM_HEIGHT, screen);
    let content_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(1),
    };
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(3),
    ])
    .split(content_area);

    ButtonGeometry {
        button_areas: compute_button_rects(&["(N)o", "(Y)es"], layout[2]),
    }
}

/// Height of the rename popup body, used by the renderer.
#[must_use]
pub fn rename_popup_height(show_overwrite_confirm: bool) -> u16 {
    if show_overwrite_confirm {
        RENAME_CONFIRM_HEIGHT
    } else {
        RENAME_HEIGHT
    }
}

const REMOTE_EDIT_WIDTH: u16 = 60;
const REMOTE_EDIT_HEIGHT: u16 = 10;

#[must_use]
pub fn remote_edit_geometry(screen: Rect, visible: bool) -> ButtonGeometry {
    if !visible {
        return ButtonGeometry::default();
    }
    let popup_area = centered_rect_absolute(REMOTE_EDIT_WIDTH, REMOTE_EDIT_HEIGHT, screen);
    let content_area = Rect {
        x: popup_area.x + 4,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(8),
        height: popup_area.height.saturating_sub(1),
    };
    let layout = Layout::vertical([
        Constraint::Length(2), // Title
        Constraint::Length(2), // File info
        Constraint::Min(1),    // Instruction
        Constraint::Length(3), // Buttons
    ])
    .split(content_area);

    ButtonGeometry {
        button_areas: compute_button_rects(&["[C]ancel", "[U]pload"], layout[3]),
    }
}

const HOST_KEY_WIDTH: u16 = 70;
const HOST_KEY_HEIGHT: u16 = 10;

#[must_use]
pub fn host_key_geometry(screen: Rect, visible: bool) -> ButtonGeometry {
    if !visible {
        return ButtonGeometry::default();
    }
    let popup_area = centered_rect_absolute(HOST_KEY_WIDTH, HOST_KEY_HEIGHT, screen);
    let content_area = Rect {
        x: popup_area.x + 2,
        y: popup_area.y + 1,
        width: popup_area.width.saturating_sub(4),
        height: popup_area.height.saturating_sub(1),
    };
    let inner_layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(3),
    ])
    .horizontal_margin(2)
    .split(content_area);

    ButtonGeometry {
        button_areas: compute_button_rects(&["(R)eject", "(A)ccept"], inner_layout[2]),
    }
}

/// Button geometry for a popup that renders the shared confirmation layout.
#[must_use]
pub fn confirmation_geometry(
    width: u16,
    height: u16,
    screen: Rect,
    visible: bool,
) -> ButtonGeometry {
    if !visible {
        return ButtonGeometry::default();
    }
    ButtonGeometry {
        button_areas: confirmation_button_areas(width, height, screen),
    }
}

/// Confirmation overlay spawned by the SSH popup.
const SSH_CONFIRM_WIDTH: u16 = 66;
const SSH_CONFIRM_HEIGHT: u16 = 6;

#[must_use]
pub fn ssh_geometry(screen: Rect, visible: bool, has_confirmation: bool) -> SshGeometry {
    if !visible {
        return SshGeometry::default();
    }
    let popup_area = centered_rect_percent(70, 80, screen);
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(6),
    ])
    .horizontal_margin(2)
    .vertical_margin(1)
    .split(popup_area);

    SshGeometry {
        field_areas: vec![chunks[0], chunks[2], chunks[4], chunks[6]],
        history_area: Some(chunks[6]),
        confirmation_buttons: if has_confirmation {
            confirmation_geometry(SSH_CONFIRM_WIDTH, SSH_CONFIRM_HEIGHT, screen, true)
        } else {
            ButtonGeometry::default()
        },
    }
}

/// Confirmation overlay spawned by the bookmark popup.
const BOOKMARK_CONFIRM_WIDTH: u16 = 60;
const BOOKMARK_CONFIRM_HEIGHT: u16 = 6;

#[must_use]
pub fn bookmark_geometry(screen: Rect, visible: bool, has_confirmation: bool) -> BookmarkGeometry {
    let list_area = filterable_list_geometry(screen, visible);
    BookmarkGeometry {
        list_area,
        confirmation_buttons: if visible && has_confirmation {
            confirmation_geometry(
                BOOKMARK_CONFIRM_WIDTH,
                BOOKMARK_CONFIRM_HEIGHT,
                screen,
                true,
            )
        } else {
            ButtonGeometry::default()
        },
    }
}

fn fuzzy_search_geometry(screen: Rect, visible: bool) -> ListGeometry {
    ListGeometry {
        list_area: filterable_list_geometry(screen, visible),
    }
}

/// Inner list area of the shared filterable-list popup (bookmark + fuzzy search).
#[must_use]
pub fn filterable_list_geometry(screen: Rect, visible: bool) -> Option<Rect> {
    if !visible {
        return None;
    }
    let popup_width =
        u16::try_from((u32::from(screen.width) * 6 / 10).min(80)).unwrap_or(screen.width);
    let popup_height =
        u16::try_from((u32::from(screen.height) * 5 / 10).min(20)).unwrap_or(screen.height);
    let popup_area = centered_rect_absolute(popup_width, popup_height, screen);

    let chunks = Layout::vertical([
        Constraint::Length(3), // Input box
        Constraint::Length(1), // Gap
        Constraint::Min(1),    // List area
    ])
    .horizontal_margin(2)
    .vertical_margin(1)
    .split(popup_area);

    // The list block draws `Borders::ALL`, so its inner area is inset by one.
    Some(chunks[2].inner(Margin {
        horizontal: 1,
        vertical: 1,
    }))
}

#[must_use]
pub fn help_geometry(screen: Rect, visible: bool) -> ListGeometry {
    if !visible {
        return ListGeometry::default();
    }
    let area = centered_rect_percent(40, 80, screen);
    let chunks = Layout::vertical([Constraint::Min(1)])
        .horizontal_margin(2)
        .vertical_margin(1)
        .split(area);

    // The inner block draws `Borders::ALL`.
    let inner_content_area = chunks[0].inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let layout = Layout::vertical([
        Constraint::Length(2), // Title
        Constraint::Min(1),    // Table
    ])
    .horizontal_margin(1)
    .split(inner_content_area);

    ListGeometry {
        list_area: Some(layout[1]),
    }
}

#[cfg(test)]
mod tests {
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
        let cases: Vec<(
            &str,
            Box<dyn Fn(&mut AppState)>,
            crate::app::PopupKind,
            &[&str],
        )> = vec![
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

        assert!(layout.error.button_areas.is_empty());
        assert!(layout.conflict.button_areas.is_empty());
        assert!(layout.rename.button_areas.is_empty());
        assert!(layout.remote_edit.button_areas.is_empty());
        assert!(layout.host_key.button_areas.is_empty());
        assert!(layout.delete.button_areas.is_empty());
        assert!(layout.empty_trash.button_areas.is_empty());
        assert!(layout.quit_confirmation.button_areas.is_empty());
        assert!(layout.ssh_connection.field_areas.is_empty());
        assert!(layout.ssh_connection.history_area.is_none());
        assert!(
            layout
                .ssh_connection
                .confirmation_buttons
                .button_areas
                .is_empty()
        );
        assert!(layout.bookmark.list_area.is_none());
        assert!(layout.bookmark.confirmation_buttons.button_areas.is_empty());
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
}
