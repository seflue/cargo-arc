use cargo_arc::{Cargo, run};
use clap::Parser;

fn main() {
    let Cargo::Arc(cmd) = Cargo::parse();
    if let Err(e) = run(cmd) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
