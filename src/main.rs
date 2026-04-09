//! Binary entry point for the `intmux` CLI.

use std::process::ExitCode;

fn main() -> ExitCode {
    match intmux::try_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
