//! Test suite for `state::file_viewer`.
mod directory_listing;
mod highlight;
mod search;
mod text;

use crate::state::FileViewerState;
use ratatui::style::Color;
use std::str::FromStr;

/// Viewer geometry for a panel of the given size, mirroring what
/// `layout::compute_layout` produces for the viewer's side.
fn viewer_geometry(width: u16, height: u16) -> crate::layout::ViewerGeometry {
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    crate::layout::ViewerGeometry {
        area,
        render_area: area.inner(ratatui::layout::Margin {
            horizontal: 1,
            vertical: 1,
        }),
    }
}

/// Renders the viewer into a test terminal and returns the screen as text.
fn render_viewer(state: &mut FileViewerState) -> String {
    render_viewer_cells(state).0
}

/// Renders the viewer and returns the screen text plus the foreground of every
/// non-blank cell inside the panel.
///
/// Colours are collected from the interior only: the border and the title are
/// drawn in their own colours and would mask what the content looks like.
fn render_viewer_cells(state: &mut FileViewerState) -> (String, Vec<ratatui::style::Color>) {
    render_viewer_cells_with_icons(state, false)
}

/// Renders the viewer with icons enabled and returns the screen as text.
fn render_viewer_with_icons(state: &mut FileViewerState) -> String {
    render_viewer_cells_with_icons(state, true).0
}

fn render_viewer_cells_with_icons(
    state: &mut FileViewerState,
    icons: bool,
) -> (String, Vec<ratatui::style::Color>) {
    let area = ratatui::layout::Rect::new(0, 0, 80, 20);
    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|f| {
            crate::ui::draw_file_viewer(
                f,
                state,
                area,
                &viewer_geometry(80, 20),
                &palette,
                true,
                icons,
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    let mut colors = Vec::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            text.push_str(buffer[(x, y)].symbol());
            let interior =
                y > area.top() && y < area.bottom() - 1 && x > area.left() && x < area.right() - 1;
            if interior && buffer[(x, y)].symbol() != " " {
                colors.push(buffer[(x, y)].fg);
            }
        }
        text.push('\n');
    }
    (text, colors)
}

/// Renders the viewer and returns the background color of every cell, indexed
/// `[y][x]`, so selection highlights can be inspected.
fn render_viewer_bg_cells(state: &mut FileViewerState, icons: bool) -> Vec<Vec<Color>> {
    let area = ratatui::layout::Rect::new(0, 0, 80, 20);
    let palette = crate::theme::get_theme("catppuccin macchiato").unwrap();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|f| {
            crate::ui::draw_file_viewer(
                f,
                state,
                area,
                &viewer_geometry(80, 20),
                &palette,
                true,
                icons,
            );
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    (0..area.height)
        .map(|y| (0..area.width).map(|x| buffer[(x, y)].bg).collect())
        .collect()
}
