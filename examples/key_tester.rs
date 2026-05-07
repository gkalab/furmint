use crossterm::event::{self, Event, KeyCode};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use std::io::{self};

fn main() -> io::Result<()> {
    println!("--- Key Event Diagnostic Tool ---");
    println!("Press any key to see its event. Press 'q' or 'Esc' to exit.");

    enable_raw_mode()?;

    loop {
        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            disable_raw_mode()?;
            println!("\r\nEvent: {key:?}");
            println!("Code: {:?}, Modifiers: {:?}", key.code, key.modifiers);

            if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                break;
            }
            enable_raw_mode()?;
        }
    }

    disable_raw_mode()?;
    println!("\r\nExited.");
    Ok(())
}
