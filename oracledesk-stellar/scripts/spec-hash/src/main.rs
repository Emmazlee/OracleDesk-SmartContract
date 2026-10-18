//! Prints the commitment hash for a resolution spec given on the command line,
//! or verifies one. See `lib.rs` for the grammar and `USAGE` for examples.

use spec_hash::{run, CliError, USAGE};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(CliError::Usage(message)) => {
            eprintln!("error: {message}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Mismatch { expected, actual }) => {
            eprintln!("mismatch: expected {expected}, computed {actual}");
            ExitCode::from(1)
        }
    }
}
