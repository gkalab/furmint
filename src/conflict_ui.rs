use crate::app::ConflictState;
use crate::theme::ThemePalette;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

pub fn draw_conflict_popup(f: &mut ratatui::Frame, state: &ConflictState, palette: &ThemePalette) {
    if !state.is_visible {
        return;
    }

    let area = f.area();
    let popup_width = 60;
    let popup_height = 8;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: popup_x,
        y: popup_y,
        width: popup_width,
        height: popup_height,
    };

    f.render_widget(Clear, popup_area);

    let bg_color = Color::Rgb(palette.base.r, palette.base.g, palette.base.b);
    let border_color = Color::Rgb(palette.red.r, palette.red.g, palette.red.b); // Red for warning
    let text_color = Color::Rgb(palette.text.r, palette.text.g, palette.text.b);

    let title = "Conflict Detected";

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title(title)
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(bg_color));

    let content = format!(
        "File already exists:\n\n{}\n",
        state.conflict_path.display()
    );

    let inner_area = block.inner(popup_area);

    let layout = Layout::vertical([
        Constraint::Min(3),    // Message + Path
        Constraint::Length(1), // Row 1 buttons
        Constraint::Length(1), // Row 2 buttons
    ])
    .split(inner_area);

    let _block_inner = Block::default();

    // We render the explanation text
    let p_text = Paragraph::new(content)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .style(Style::default().fg(text_color));
    f.render_widget(p_text, layout[0]);

    // Draw block/borders first
    f.render_widget(&block, popup_area);
    let mut inner_area = block.inner(popup_area);
    inner_area.x += 1;
    inner_area.width = inner_area.width.saturating_sub(2);
    let layout = Layout::vertical([
        Constraint::Min(3),    // Message + Path
        Constraint::Length(1), // Row 1 buttons
        Constraint::Length(1), // Row 2 buttons
    ])
    .split(inner_area);

    crate::ui_utils::draw_button_row(
        f,
        &["[O]verwrite", "[S]kip", "[C]ancel"],
        layout[1],
        text_color,
    );

    crate::ui_utils::draw_button_row(
        f,
        &["Overwrite [Y]All", "Skip [A]ll"],
        layout[2],
        text_color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    #[test]
    fn test_draw_conflict_popup_visible_does_not_panic() {
        // Setup test backend and Frame
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = crate::state::ConflictState {
                    is_visible: true,
                    task_id: 42,
                    conflict_path: PathBuf::from("/tmp/existing.txt"),
                    conflict_type: crate::tasks::ConflictType::FileExists,
                };
                let palette = crate::theme::default_theme();
                // Should not panic
                draw_conflict_popup(f, &state, &palette);
            })
            .unwrap();
    }

    #[test]
    fn test_draw_conflict_popup_invisible_does_nothing() {
        // Setup test backend and Frame
        let backend = TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let state = crate::state::ConflictState::default(); // is_visible = false
                let palette = crate::theme::default_theme();
                // Should just return, nothing rendered
                draw_conflict_popup(f, &state, &palette);
            })
            .unwrap();
    }
}
