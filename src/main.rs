use cargo_arc::{Cargo, run};
use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Cargo::Arc(cmd) = Cargo::parse();
    match run(cmd) {
        Ok(judgment) => judgment.exit_code(),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
