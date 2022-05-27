use anyhow::{anyhow, Result};
use std::sync::{mpsc::Sender, Arc};
use time::{format_description, UtcOffset};

use clap::Parser;
use cursive::{
    event::{Event, Key},
    view::SizeConstraint,
    views::{Dialog, DummyView, LinearLayout, ResizedView, ScrollView, TextContent, TextView},
    View,
};
use log::{error, info};
use mpv::DEFAULT_VOL;

mod mpv;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    #[clap()]
    args: Vec<String>,
}

pub enum VanControl {
    SetVolume(f64),
    NextSong,
    PrevSong,
}

fn main() {
    cursive::logger::init();
    log::set_max_level(log::LevelFilter::Info);
    let mut siv = cursive::default();
    let (control_tx, control_rx) = std::sync::mpsc::channel();

    let mut vol_view = TextView::new(format!("vol: {}", DEFAULT_VOL));
    let vol_status = Arc::new(vol_view.get_shared_content());

    let mut current_song_view = TextView::new("Unknown");
    let current_song_status = Arc::new(current_song_view.get_shared_content());

    let mut current_time_view = TextView::new("-/-");
    let current_time_status = Arc::new(current_time_view.get_shared_content());

    std::thread::spawn(move || {
        let (getinfo_tx, getinfo_rx) = std::sync::mpsc::channel();
        let current_song_status_clone = current_song_status.clone();
        std::thread::spawn(move || {
            // FIXME: Non-C locale detected. This is not supported.
            // Call 'setlocale(LC_NUMERIC, "C");' in your code.
            let buf = std::ffi::CString::new("C").expect("Unknown Error!");
            unsafe { libc::setlocale(libc::LC_NUMERIC, buf.as_ptr()) };
            if let Err(e) = mpv::play(control_rx, getinfo_tx) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        });
        loop {
            let mut s = String::from("-/-");
            if let Ok(m) = getinfo_rx.try_recv() {
                info!("Recviver! {:?}", m);
                current_song_status_clone.set_content(m.title);
                if let Ok(current_time) = get_time(m.current_time) {
                    s = s.replace("-/", &format!("{}/", current_time));
                }
                if let Ok(duration) = get_time(m.duration) {
                    s = s.replace("/-", &format!("/{}", duration));
                }
                current_time_status.set_content(s);
            }
        }
    });

    let args = Args::parse().args;
    for i in args {
        if let Err(e) = mpv::add(&i) {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }

    let view = wrap_in_dialog(
        LinearLayout::vertical()
            .child(current_song_view.center())
            .child(DummyView {})
            .child(current_time_view.center())
            .child(DummyView {})
            .child(vol_view.center()),
        "Van",
        None,
    );
    let volume_status_clone = vol_status.clone();
    let control_tx_clone = control_tx.clone();
    let control_tx_clone_2 = control_tx.clone();
    let control_tx_clone_3 = control_tx.clone();

    siv.add_global_callback('=', move |_| {
        if let Err(e) = add_volume(control_tx.clone(), volume_status_clone.clone()) {
            error!("{}", e);
        }
    });
    siv.add_global_callback('-', move |_| {
        if let Err(e) = reduce_volume(control_tx_clone_2.clone(), vol_status.clone()) {
            error!("{}", e);
        }
    });
    siv.add_global_callback(Event::Key(Key::Right), move |_| {
        control_tx_clone_3.send(VanControl::NextSong).unwrap();
    });
    siv.add_global_callback(Event::Key(Key::Left), move |_| {
        control_tx_clone.send(VanControl::PrevSong).unwrap();
    });

    siv.add_global_callback('~', cursive::Cursive::toggle_debug_console);

    siv.set_autorefresh(true);
    siv.add_layer(view);
    siv.run();
}

fn add_volume(control_tx: Sender<VanControl>, vol_status: Arc<TextContent>) -> Result<()> {
    let mut current_vol = mpv::get_volume()?;
    if current_vol < 100.0 {
        current_vol += 5.0;
        control_tx.send(VanControl::SetVolume(current_vol))?;
        vol_status.set_content(format!("vol: {}", current_vol));
    }

    Ok(())
}

fn reduce_volume(control_tx: Sender<VanControl>, vol_status: Arc<TextContent>) -> Result<()> {
    let mut current_vol = mpv::get_volume()?;
    if current_vol > 0.0 {
        current_vol -= 5.0;
        control_tx.send(VanControl::SetVolume(current_vol))?;
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

fn get_time(time: i64) -> Result<String> {
    let f = format_description::parse("[offset_minute]:[offset_second]")?;
    let offset = UtcOffset::from_whole_seconds(time.try_into()?)?;
    let minute = offset.whole_minutes();
    let date = offset.format(&f)?;
    let sess = date
        .split_once(':')
        .map(|x| x.1)
        .ok_or_else(|| anyhow!("Can not convert time!"))?;
    let date = format!("{}:{}", minute, sess);

    Ok(date)
}

#[test]
fn test_time() {
    assert_eq!(get_time(3601).unwrap(), "60:01");
    assert_eq!(get_time(1).unwrap(), "0:01");
}
