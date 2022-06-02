use cursive::{Cursive, CursiveExt};
use clap::Parser;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    #[clap()]
    args: Vec<String>,
}

fn main() {
    let mut siv = Cursive::default();

    let args = Args::parse().args;
    if let Err(e) = van_core::init_siv(&mut siv, args) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    siv.run();
}
