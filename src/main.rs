use std::sync::{mpsc::Sender, Arc};
use anyhow::Result;

use cursive::{
    view::SizeConstraint,
    views::{Dialog, LinearLayout, ResizedView, ScrollView, TextView, TextContent, DummyView},
    View, event::{Event, Key}, Cursive
};
use log::info;
use van_player::mpv::DEFAULT_VOL;

mod mpv;
mod youtubedl;

fn main() {
    cursive::logger::init();
    log::set_max_level(log::LevelFilter::Info);
    let mut siv = cursive::default();
    let (volume_tx, volume_rx) = std::sync::mpsc::channel();
    let (next_tx, next_rx) = std::sync::mpsc::channel();
    let (prev_tx, prev_rx) = std::sync::mpsc::channel();

    let mut vol_view = TextView::new(format!("vol: {}", DEFAULT_VOL));
    let vol_status = Arc::new(vol_view.get_shared_content());

    let mut current_song_view = TextView::new("Unknown");
    let current_song_status = Arc::new(current_song_view.get_shared_content());

    std::thread::spawn(move || {
        let (getinfo_tx, getinfo_rx) = std::sync::mpsc::channel();
        let current_song_status_clone = current_song_status.clone();
        std::thread::spawn(move || {
            // FIXME: Non-C locale detected. This is not supported.
            // Call 'setlocale(LC_NUMERIC, "C");' in your code.
            let buf = std::ffi::CString::new("C").expect("Unknown Error!");
            unsafe { libc::setlocale(libc::LC_NUMERIC, buf.as_ptr()) };
            if let Err(e) = mpv::play(volume_rx, next_rx, prev_rx, getinfo_tx) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        });
        loop {
            if let Ok(m) = getinfo_rx.try_recv() {
                info!("Recviver! {:?}", m);
                current_song_status_clone.set_content(m.title);
            }
        }
    });
    if let Err(e) = mpv::add("https://www.bilibili.com/video/BV1WL4y1F7Uj") {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    if let Err(e) = mpv::add("https://www.bilibili.com/video/BV19U4y13777") {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    let view = wrap_in_dialog(
        LinearLayout::vertical().child(current_song_view).child(DummyView {}).child(vol_view),
        "Van",
        None,
    );
    let volume_tx_clone = volume_tx.clone();
    let volume_tx_clone_2 = volume_tx.clone();
    let volume_status_clone = vol_status.clone();
    let volume_status_clone_2 = vol_status.clone();

    siv.add_global_callback('=', move |_| {
        add_volume(volume_tx_clone.clone(), volume_status_clone.clone()).ok();
    });
    siv.add_global_callback('-', move |_| {
        reduce_volume(volume_tx_clone_2.clone(), volume_status_clone_2.clone()).ok();
    });
    siv.add_global_callback(Event::Key(Key::Right), move |_| {
        next_tx.send(true).unwrap();
    });
    siv.add_global_callback('~', cursive::Cursive::toggle_debug_console);

    siv.set_autorefresh(true);
    siv.add_layer(view);
    siv.run();
}

fn add_volume(volume_tx: Sender<f64>, vol_status: Arc<TextContent>) -> Result<()> {
    let mut current_vol = mpv::get_volume()?;
    if current_vol < 100.0 {
        current_vol += 5.0;
        volume_tx.send(current_vol)?;
        vol_status.set_content(format!("vol: {}", current_vol));
    }

    Ok(())
}

fn reduce_volume(volume_tx: Sender<f64>, vol_status: Arc<TextContent>) -> Result<()> {
    let mut current_vol = mpv::get_volume()?;
    if current_vol > 0.0 {
        current_vol -= 5.0;
        volume_tx.send(current_vol)?;
        vol_status.set_content(format!("vol: {}", current_vol));
    }

    Ok(())
}

fn wrap_in_dialog<V: View, S: Into<String>>(inner: V, title: S, width: Option<usize>) -> Dialog {
    Dialog::around(ResizedView::new(
        SizeConstraint::AtMost(width.unwrap_or(64)),
        SizeConstraint::Free,
        ScrollView::new(inner),
    ))
    .padding_lrtb(2, 2, 1, 1)
    .title(title)
}
