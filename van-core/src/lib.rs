mod mpv;

use anyhow::{anyhow, Result};
use std::sync::{
    mpsc::{Receiver, Sender},
    Arc
};
use time::{format_description, UtcOffset};

use cursive::{
    event::{Event, Key},
    view::SizeConstraint,
    views::{
        Dialog, DummyView, LinearLayout, ResizedView, ScrollView, SelectView, TextContent, TextView,
    },
    Cursive, View,
};
use log::{error};
use mpv::{Van, DEFAULT_VOL};
use mpv::VanControl;

use mpv::PlayStatus;

struct CurrentStatus {
    vol: Option<Arc<TextContent>>,
    current_song_status: Arc<TextContent>,
    current_artist_status: Arc<TextContent>,
    current_time_status: Arc<TextContent>,
}

/// Init Cursive view
/// ```rust
/// use cursive::{Cursive, CursiveExt};
///
///
/// let mut siv = Cursive::default();
///
/// let (control_tx, control_rx) = std::sync::mpsc::channel();
/// if let Err(e) = van_core::init_siv(&mut siv, vec!["https://www.bilibili.com/video/BV1HB4y1175c".to_string()], control_tx, control_rx) {
///     eprintln!("{}", e);
///     std::process::exit(1);
/// }
///
/// siv.run();
/// ```
pub fn init_siv(
    siv: &mut Cursive,
    args: Vec<String>,
    control_tx: Sender<VanControl>,
    control_rx: Receiver<VanControl>,
) -> Result<()> {
    let (view, current_status) = get_view();
    let vol_status = current_status.vol.unwrap();
    let van = Arc::new(Van::new()?);
    let van_clone = van.clone();
    let van_clone_2 = van.clone();

    for i in args {
        van.add(&i)?;
    }

    start_mpv(
        CurrentStatus {
            vol: None,
            ..current_status
        },
        control_rx,
        van_clone
    );

    set_cursive(vol_status, control_tx, siv, van_clone_2);

    siv.add_layer(view);

    Ok(())
}

/// Destroy mpv
pub fn destroy_mpv(control_tx: Sender<VanControl>) -> Result<()> {
    control_tx.send(VanControl::Exit)?;

    Ok(())
}

fn set_cursive(vol_status: Arc<TextContent>, control_tx: Sender<VanControl>, siv: &mut Cursive, van: Arc<Van>) {
    let volume_status_clone = vol_status.clone();
    let control_tx_clone = control_tx.clone();
    let control_tx_clone_2 = control_tx.clone();
    let control_tx_clone_3 = control_tx.clone();
    let control_tx_clone_4 = control_tx.clone();
    let van_clone = van.clone();
    let van_clone_2 = van.clone();
    let van_clone_3 = van.clone();



    siv.add_global_callback('=', move |_| {
        if let Err(e) = add_volume(control_tx.clone(), volume_status_clone.clone(), van_clone.clone()) {
            error!("{}", e);
        }
    });
    siv.add_global_callback('-', move |_| {
        if let Err(e) = reduce_volume(control_tx_clone_2.clone(), vol_status.clone(), van_clone_2.clone()) {
            error!("{}", e);
        }
    });
    siv.add_global_callback(Event::Key(Key::Right), move |_| {
        control_tx_clone_3.send(VanControl::NextSong).unwrap();
    });
    siv.add_global_callback(Event::Key(Key::Left), move |_| {
        control_tx_clone.send(VanControl::PrevSong).unwrap();
    });
    siv.add_global_callback('p', move |_| {
        control_tx_clone_4.send(VanControl::PauseControl).unwrap();
    });
    siv.add_global_callback('l', move |s| {
        playlist_view(s, van_clone_3.clone());
    });
    siv.add_global_callback('~', cursive::Cursive::toggle_debug_console);
    siv.set_autorefresh(true);
}

fn get_view() -> (Dialog, CurrentStatus) {
    let mut vol_view = TextView::new(format!("vol: {}", DEFAULT_VOL));
    let vol_status = Arc::new(vol_view.get_shared_content());

    let mut current_song_view = TextView::new("Unknown");
    let current_song_status = Arc::new(current_song_view.get_shared_content());

    let mut current_time_view = TextView::new("-/-");
    let current_time_status = Arc::new(current_time_view.get_shared_content());

    let mut current_artist_view = TextView::new("Unknown");
    let current_artist_status = Arc::new(current_artist_view.get_shared_content());

    let view = wrap_in_dialog(
        LinearLayout::vertical()
            .child(current_song_view.center())
            .child(DummyView {})
            .child(current_artist_view.center())
            .child(DummyView {})
            .child(current_time_view.center())
            .child(DummyView {})
            .child(vol_view.center()),
        "Van",
        None,
    );

    (
        view,
        CurrentStatus {
            vol: Some(vol_status),
            current_song_status,
            current_artist_status,
            current_time_status,
        },
    )
}

fn playlist_view(siv: &mut Cursive, van: Arc<Van>) {
    let playlist = van.get_playlist();
    let mut files = vec![];
    if let Ok(playlist) = playlist {
        for i in playlist {
            files.push(i.filename);
        }
    } else {
        error!("{:?}", playlist.unwrap_err());
    }
    let view = wrap_in_dialog(
        SelectView::new()
            .with_all_str(files.clone())
            .on_submit(move |s, c: &String| {
                let index = files.clone().iter().position(|x| x == c);
                if let Some(index) = index {
                    van.force_play(index.try_into().unwrap()).ok();
                }
                s.pop_layer();
            }),
        "Playlist",
        None,
    )
    .button("Back", |s| {
        s.cb_sink()
            .send(Box::new(|s| {
                s.pop_layer();
            }))
            .unwrap();
    });
    siv.add_layer(view);
}

fn start_mpv(current_status: CurrentStatus, control_rx: Receiver<VanControl>, van: Arc<Van>) {
    let van_clone = van.clone();
    std::thread::spawn(move || {
        let (getinfo_tx, getinfo_rx) = std::sync::mpsc::channel();
        let current_song_status_clone = current_status.current_song_status.clone();
        std::thread::spawn(move || {
            // FIXME: Non-C locale detected. This is not supported.
            // Call 'setlocale(LC_NUMERIC, "C");' in your code.
            let buf = std::ffi::CString::new("C").expect("Unknown Error!");
            unsafe { libc::setlocale(libc::LC_NUMERIC, buf.as_ptr()) };
            if let Err(e) = van.play(control_rx, getinfo_tx) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
        });
        loop {
            let mut time_str = String::from("-/-");
            let r = getinfo_rx.try_recv();
            if let Ok(status) = r {
                // info!("Recviver! {:?}", status);
                match status {
                    PlayStatus::MediaInfo(m) => {
                        current_song_status_clone.set_content(m.title);
                        current_status.current_artist_status.set_content(m.artist);
                        if let Ok(current_time) = get_time(m.current_time) {
                            time_str = time_str.replace("-/", &format!("{}/", current_time));
                        }
                        if let Ok(duration) = get_time(m.duration) {
                            time_str = time_str.replace("/-", &format!("/{}", duration));
                        }
                        current_status.current_time_status.set_content(time_str);
                    }
                    PlayStatus::Loading => {
                        if let Ok(name) = van_clone.get_file_name() {
                            current_status.current_song_status.clone().set_content(name);
                            current_status
                                .current_artist_status
                                .clone()
                                .set_content("Unknown");
                            current_status
                                .current_time_status
                                .clone()
                                .set_content("-/-");
                        }
                    }
                }
            }
        }
    });
}

fn add_volume(control_tx: Sender<VanControl>, vol_status: Arc<TextContent>, van: Arc<Van>) -> Result<()> {
    let mut current_vol = van.get_volume()?;
    if current_vol < 100.0 {
        current_vol += 5.0;
        control_tx.send(VanControl::SetVolume(current_vol))?;
        vol_status.set_content(format!("vol: {}", current_vol));
    }

    Ok(())
}

fn reduce_volume(control_tx: Sender<VanControl>, vol_status: Arc<TextContent>, van: Arc<Van>) -> Result<()> {
    let mut current_vol = van.get_volume()?;
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
