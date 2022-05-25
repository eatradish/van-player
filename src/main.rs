use std::{sync::{mpsc::Sender, Arc}, fmt::format};
use anyhow::Result;

use cursive::{
    view::SizeConstraint,
    views::{Dialog, LinearLayout, ResizedView, ScrollView, TextView, TextContent},
    View,
};
use van_player::mpv::{get_volume, DEFAULT_VOL};

mod mpv;
mod youtubedl;

fn main() {
    let mut siv = cursive::default();
    let (volume_tx, volume_rx) = std::sync::mpsc::channel();
    let (next_tx, next_rx) = std::sync::mpsc::channel();
    let (prev_tx, prev_rx) = std::sync::mpsc::channel();
    let (get_info_tx, get_info_rx) = std::sync::mpsc::channel();
    std::thread::spawn(|| {
        // FIXME: Non-C locale detected. This is not supported.
        // Call 'setlocale(LC_NUMERIC, "C");' in your code.
        let buf = std::ffi::CString::new("C").expect("Unknown Error!");
        unsafe { libc::setlocale(libc::LC_NUMERIC, buf.as_ptr()) };
        if let Err(e) = mpv::play(volume_rx, next_rx, prev_rx, get_info_tx) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    });
    if let Err(e) = mpv::add("https://www.bilibili.com/video/BV1WL4y1F7Uj") {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    let mut vol_view = TextView::new(format!("vol: {}", DEFAULT_VOL));
    let vol_status = Arc::new(vol_view.get_shared_content());

    let view = wrap_in_dialog(
        LinearLayout::vertical().child(vol_view),
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
    siv.add_layer(view);
    siv.run()
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
