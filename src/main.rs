use clap::Parser;
use cursive::{Cursive, CursiveExt, reexports::log};
use van_core::destroy_mpv;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    #[clap()]
    args: Vec<String>,
}

fn main() {
    cursive::logger::init();
    log::set_max_level(log::LevelFilter::Info);
    let mut siv = Cursive::default();

    let (control_tx, control_rx) = std::sync::mpsc::channel();
    let control_tx_clone = control_tx.clone();
    let args = Args::parse().args;
    if let Err(e) = van_core::init_siv(&mut siv, args, control_tx, control_rx) {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    siv.add_global_callback('q', move |_| {
        destroy_mpv(control_tx_clone.clone()).unwrap();
    });

    siv.run();
}
