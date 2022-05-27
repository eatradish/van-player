use lazy_static::lazy_static;
use libmpv::{events::*, *};
use log::{error, info};
use std::{
    collections::HashMap,
    sync::{
        mpsc::{Receiver, Sender},
        Mutex,
    },
};

use anyhow::{anyhow, Result};

#[derive(Debug)]
pub struct MediaInfo {
    pub title: String,
    pub duration: i64,
    pub current_time: i64,
}

pub const DEFAULT_VOL: f64 = 50.0;

lazy_static! {
    pub static ref MPV: Mpv = Mpv::new().expect("Can not init mpv");
    pub static ref QUEUE: Mutex<Vec<(String, FileState, Option<&'static str>)>> =
        Mutex::new(Vec::new());
}

macro_rules! check_err {
    ($i:expr, $err_tx:expr) => {
        if let Err(e) = $i {
            $err_tx.send(e.to_string()).unwrap();
            error!("{}", e);
            return;
        }
    };
}

pub fn add(url: &str) -> Result<()> {
    let mut queue = QUEUE.lock().map_err(|e| anyhow!("{}", e))?;
    queue.push((url.to_string(), FileState::AppendPlay, None));

    Ok(())
}

pub fn play(
    vol_rx: Receiver<f64>,
    control_rx: Receiver<bool>,
    getinfo_tx: Sender<MediaInfo>,
) -> Result<()> {
    MPV.set_property("volume", DEFAULT_VOL)
        .map_err(|e| anyhow!("{}", e))?;
    play_inner(vol_rx, control_rx, getinfo_tx)?;

    Ok(())
}

pub fn get_volume() -> Result<f64> {
    MPV.get_property::<f64>("volume")
        .map_err(|e| anyhow!("{}", e))
}

fn seekable_ranges(demuxer_cache_state: &MpvNode) -> Option<Vec<(f64, f64)>> {
    let mut res = Vec::new();
    let props: HashMap<&str, MpvNode> = demuxer_cache_state.to_map()?.collect();
    let ranges = props.get("seekable-ranges")?.to_array()?;

    for node in ranges {
        let range: HashMap<&str, MpvNode> = node.to_map()?.collect();
        let start = range.get("start")?.to_f64()?;
        let end = range.get("end")?.to_f64()?;
        res.push((start, end));
    }

    Some(res)
}

fn get_current_song_index() -> Result<i64> {
    let current = MPV
        .get_property::<i64>("playlist-pos")
        .map_err(|e| anyhow!("{}", e))?;

    Ok(current)
}

fn get_total_content() -> Result<i64> {
    MPV.get_property::<i64>("playlist-count")
        .map_err(|e| anyhow!("{}", e))
}

fn play_inner(
    vol_rx: Receiver<f64>,
    song_control_rx: Receiver<bool>,
    getinfo_tx: Sender<MediaInfo>,
) -> Result<()> {
    let mut ev_ctx = MPV.create_event_context();
    ev_ctx
        .observe_property("volume", Format::Int64, 0)
        .map_err(|e| anyhow!("{}", e))?;
    ev_ctx
        .observe_property("demuxer-cache-state", Format::Node, 0)
        .map_err(|e| anyhow!("{}", e))?;

    let (err_tx, err_rx) = std::sync::mpsc::channel();
    let err_tx_2 = err_tx.clone();
    let err_tx_3 = err_tx.clone();

    crossbeam::scope(|scope| {
        scope.spawn(move |_| {
            check_err!(MPV.set_property("vo", "null"), err_tx);
            let queue = &*QUEUE.lock().unwrap();
            let queue = queue
                .iter()
                .map(|(x, y, z)| (x.as_str(), *y, *z))
                .collect::<Vec<_>>();
            check_err!(MPV.playlist_load_files(&queue), err_tx);
        });
        scope.spawn(move |_| loop {
            if let Ok(v) = vol_rx.try_recv() {
                check_err!(MPV.set_property("volume", v), err_tx_2);
            }
            if let Ok(next) = song_control_rx.try_recv() {
                if next {
                    check_err!(MPV.playlist_next_weak(), err_tx_2);
                } else {
                    check_err!(MPV.playlist_previous_weak(), err_tx_2);
                }
            }
        });
        scope.spawn(move |_| loop {
            let ev = ev_ctx.wait_event(600.).unwrap_or(Err(Error::Null));
            info!("{:?}", get_current_song_index());
            match ev {
                Ok(Event::PropertyChange {
                    name: "demuxer-cache-state",
                    change: PropertyData::Node(node),
                    ..
                }) => {
                    let current_media = get_current_media_info();
                    if let Ok(m) = current_media {
                        info!("Send! {:?}", m);
                        getinfo_tx.send(m).ok();
                    }
                    info!("{:?}", seekable_ranges(node));
                }
                Ok(Event::Deprecated(_)) => {
                    if get_total_content().ok() == get_current_song_index().ok().map(|x| x + 1) {
                        let queue = QUEUE.lock().unwrap();
                        let queue_ref = queue
                            .iter()
                            .map(|(x, y, z)| (x.as_str(), *y, *z))
                            .collect::<Vec<_>>();

                        check_err!(MPV.playlist_load_files(&queue_ref), err_tx_3);
                    }
                }
                _ => continue,
            }
        });
    })
    .map_err(|e| anyhow!("{:?}", e))?;

    if let Ok(e) = err_rx.recv() {
        return Err(anyhow!("{}", e));
    }

    Ok(())
}

pub fn get_current_media_info() -> Result<MediaInfo> {
    let title = MPV
        .get_property("media-title")
        .map_err(|e| anyhow!("{}", e))?;
    let duration = MPV
        .get_property::<i64>("duration")
        .map_err(|e| anyhow!("{}", e))?;
    let current_time = MPV
        .get_property::<i64>("time-pos")
        .map_err(|e| anyhow!("{}", e))?;

    Ok(MediaInfo {
        title,
        duration,
        current_time,
    })
}

#[test]
fn test_play() {
    use std::time::Duration;
    let (tx, rx) = std::sync::mpsc::channel();
    let (control_tx, control_rx) = std::sync::mpsc::channel();
    let (getinfo_tx, getinfo_rx) = std::sync::mpsc::channel();
    add("https://www.bilibili.com/video/BV1gr4y1b7vN").unwrap();
    add("https://www.bilibili.com/video/BV1GR4y1w7CR").unwrap();
    add("https://www.bilibili.com/video/BV133411V7dY").unwrap();
    add("https://www.bilibili.com/video/BV1WL4y1F7Uj").unwrap();
    add("https://www.bilibili.com/video/BV18B4y127QA").unwrap();
    add("https://www.bilibili.com/video/BV1NY4y1t7hx?p=7").unwrap();
    let work = std::thread::spawn(|| {
        play(rx, control_rx, getinfo_tx).unwrap();
    });
    tx.send(50.0).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    dbg!(getinfo_rx.recv().ok());
    control_tx.send(true).unwrap();
    dbg!(get_current_song_index().unwrap());
    std::thread::sleep(Duration::from_secs(10));
    control_tx.send(true).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    dbg!(get_current_song_index().unwrap());
    control_tx.send(false).unwrap();
    work.join().unwrap();
}
