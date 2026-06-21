use std::io::{self, Write as _};
use termina::event::{Event, KeyCode};
use termina::{PlatformTerminal, Terminal};

fn main() -> io::Result<()> {
    let mut terminal = PlatformTerminal::new()?;
    terminal.enter_raw_mode()?;

    writeln!(terminal, "--- Key Event Diagnostic Tool ---")?;
    writeln!(
        terminal,
        "Press any key to see its event. Press 'q' or 'Esc' to exit."
    )?;
    terminal.flush()?;

    loop {
        if terminal.poll(|_| true, Some(std::time::Duration::from_millis(100)))? {
            if let Event::Key(key) = terminal.read(|_| true)? {
                writeln!(terminal, "\r\nEvent: {key:?}")?;
                writeln!(
                    terminal,
                    "Code: {:?}, Modifiers: {:?}",
                    key.code, key.modifiers
                )?;
                terminal.flush()?;

                if key.code == KeyCode::Char('q') || key.code == KeyCode::Escape {
                    break;
                }
            }
        }
    }

    terminal.enter_cooked_mode()?;
    println!("\r\nExited.");
    Ok(())
}
