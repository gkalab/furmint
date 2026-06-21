use anyhow::Result;
use fm::config::create_default_config;
use fm::run;
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let bin_name = args.first().map_or("fm", std::string::String::as_str);

    if args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        println!("fm - A TUI file manager");
        println!();
        println!("Usage: {bin_name} [OPTIONS]");
        println!();
        println!("Options:");
        println!("  -h, --help            Show this help message");
        println!("  --version             Show version information");
        println!("  --create-config       Create default configuration file");
        println!();
        println!("Key bindings (default):");
        println!("  F1                  Show help screen");
        println!("  Ctrl-q              Quit");
        println!("  Arrow keys          Navigate");
        println!("  Enter               Enter directory / Open file");
        println!("  Tab                 Change panel");
        return Ok(());
    }
    if args.contains(&"--version".to_string()) {
        println!("fm version {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if args.contains(&"--create-config".to_string()) {
        match create_default_config() {
            Ok(path) => {
                println!("Default configuration created at: {}", path.display());
                return Ok(());
            }
            Err(e) => {
                eprintln!("Error creating default config: {e}");
                std::process::exit(1);
            }
        }
    }

    let result = run().await;

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    std::process::exit(0);
}
