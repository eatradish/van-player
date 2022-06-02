use clap::Parser;
use cursive::{Cursive, CursiveExt};

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    #[clap()]
    args: Vec<String>,
}

fn main() {
    let mut siv = Cursive::default();

    let (control_tx, control_rx) = std::sync::mpsc::channel();
    let args = Args::parse().args;
    if let Err(e) = van_core::init_siv(&mut siv, args, control_tx, control_rx) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    siv.run();
}
