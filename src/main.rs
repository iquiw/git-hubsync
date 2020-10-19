use std::process::exit;

use colored::Colorize;

mod err;
#[macro_use]
mod git;
mod hubsync;

fn main() {
    if let Err(e) = hubsync::hubsync() {
        eprintln!("{}: {}", "fatal".bright_red(), e);
        exit(1);
    }
}
