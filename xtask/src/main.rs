//! Development tasks for sound-to-serum. Run them with `cargo xtask`.

use clap::Parser;

/// Development tasks for sound-to-serum.
#[derive(Debug, Parser)]
#[command(name = "cargo xtask", long_about = None, arg_required_else_help = true)]
struct Xtask {}

fn main() {
    let _xtask = Xtask::parse();
}
