use std::process::exit;

mod err;
#[macro_use]
mod git;
mod hubsync;

fn main() {
    if let Err(e) = hubsync::hubsync() {
        eprintln!("git-hubsync: {}", e);
        exit(1);
    }
}
