use cursive::{Cursive, CursiveExt};

fn main() {
    let mut siv = Cursive::default();
    if let Err(e) = van_player::init_siv(&mut siv) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    siv.run();
}
