//! Diagnostic key probe: shows exactly how *your* terminal delivers keys.
//!
//! Run it with:
//!
//! ```sh
//! cargo run --example key_probe
//! ```
//!
//! Then press, in order:
//!   Home, End, Shift+Home, Shift+End, Ctrl+Home, Ctrl+End,
//!   Shift+Left, Ctrl+Left, Shift+Ctrl+Left
//!
//! Quit with Esc or Ctrl-q.
//!
//! What to look for: if Shift+Home prints `modifiers=SHIFT`, the terminal and
//! the parser are delivering the modifier correctly and the app can act on it.
//! If it prints `modifiers=NONE` (the same as plain Home), then the terminal
//! itself is not reporting the Shift modifier, so no application can tell
//! Shift+Home apart from Home.

use std::io::Write;

use ratatui::termina::{PlatformTerminal, Terminal as _};
use termina::event::{Event, KeyCode, KeyEventKind, Modifiers};

fn main() -> std::io::Result<()> {
    let mut terminal = PlatformTerminal::new()?;
    terminal.enter_raw_mode()?;
    let reader = terminal.event_reader();

    println!("Key probe — press keys to see how this terminal decodes them.");
    println!("Try: Home, End, Shift+Home, Shift+End, Ctrl+Home, Ctrl+End.");
    println!("Quit with Esc or Ctrl-q.\r");

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    loop {
        let Event::Key(key) = reader.read(|_| true)? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        let shift = key.modifiers.contains(Modifiers::SHIFT);
        let ctrl = key.modifiers.contains(Modifiers::CONTROL);
        writeln!(
            out,
            "code={:?} modifiers={:?} (shift={shift} ctrl={ctrl})\r",
            key.code, key.modifiers
        )?;
        out.flush()?;

        if key.code == KeyCode::Escape
            || (key.code == KeyCode::Char('q') && key.modifiers.contains(Modifiers::CONTROL))
        {
            break;
        }
    }

    drop(out);
    terminal.enter_cooked_mode()?;
    println!("\rDone.\r");
    Ok(())
}
