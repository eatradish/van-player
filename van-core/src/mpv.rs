use anyhow::{anyhow, Result};
use lazy_static::lazy_static;
use libmpv::{events::*, *};
use log::{error, info};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{
        mpsc::{Receiver, Sender},
        Mutex,
    },
};

pub enum VanControl {
    SetVolume(f64),
    NextSong,
    PrevSong,
    PauseControl,
    Exit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaInfo {
    pub title: String,
    pub artist: String,
    pub duration: i64,
    pub current_time: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlayStatus {
    MediaInfo(MediaInfo),
    Loading,
}

#[derive(Debug, Deserialize)]
pub struct PlayListItem {
    pub filename: String,
    pub current: Option<bool>,
    pub playing: Option<bool>,
    pub id: i32,
}

pub const DEFAULT_VOL: f64 = 50.0;

lazy_static! {
    static ref MPV: Mpv = Mpv::new().expect("Can not init mpv");
    static ref QUEUE: Mutex<Vec<(String, FileState, Option<&'static str>)>> =
        Mutex::new(Vec::new());
}

macro_rules! check_err {
    ($i:expr, $err_tx:expr) => {
        if let Err(e) = $i {
            $err_tx.send(e.to_string()).unwrap();
            error!("{}", e);
        }
    };
}

pub fn add(url: &str) -> Result<()> {
    let mut queue = QUEUE.lock().map_err(|e| anyhow!("{}", e))?;
    queue.push((url.to_string(), FileState::AppendPlay, None));

    Ok(())
}

pub fn play(control_rx: Receiver<VanControl>, getinfo_tx: Sender<PlayStatus>) -> Result<()> {
    MPV.set_property("volume", DEFAULT_VOL)
        .map_err(|e| anyhow!("{}", e))?;
    play_inner(control_rx, getinfo_tx)?;

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
        .get_property("playlist-pos")
        .map_err(|e| anyhow!("{}", e))?;

    Ok(current)
}

fn get_total_content() -> Result<i64> {
    MPV.get_property("playlist-count")
        .map_err(|e| anyhow!("{}", e))
}

pub fn get_file_name() -> Result<String> {
    MPV.get_property("filename").map_err(|e| anyhow!("{}", e))
}

pub fn force_play(count: i64) -> Result<()> {
    MPV.set_property("playlist-pos", count)
        .map_err(|e| anyhow!("{}", e))?;

    Ok(())
}

fn play_inner(song_control_rx: Receiver<VanControl>, getinfo_tx: Sender<PlayStatus>) -> Result<()> {
    let mut ev_ctx = MPV.create_event_context();
    let (exit_tx, exit_rx) = std::sync::mpsc::channel();
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
            check_err!(
                MPV.set_property("options/ytdl-raw-options", "yes-playlist="),
                err_tx
            );
            check_err!(MPV.set_property("vo", "null"), err_tx);
            let queue = &*QUEUE.lock().unwrap();
            let queue = queue
                .iter()
                .map(|(x, y, z)| (x.as_str(), *y, *z))
                .collect::<Vec<_>>();
            check_err!(MPV.playlist_load_files(&queue), err_tx);
        });
        scope.spawn(move |_| loop {
            // info!("{:?}", MPV.get_property::<String>("playlist"));
            let current_media = get_current_media_info();
            if let Ok(m) = current_media {
                getinfo_tx.send(PlayStatus::MediaInfo(m.clone())).ok();
                // info!("Send! {:?}", m);
            } else {
                getinfo_tx.send(PlayStatus::Loading).ok();
                // info!("Send! Control song");
            }
            if let Ok(v) = song_control_rx.try_recv() {
                match v {
                    VanControl::SetVolume(vol) => {
                        check_err!(MPV.set_property("volume", vol), err_tx_2)
                    }
                    VanControl::NextSong => {
                        getinfo_tx.send(PlayStatus::Loading).ok();
                        check_err!(MPV.playlist_next_weak(), err_tx_2)
                    }
                    VanControl::PrevSong => {
                        getinfo_tx.send(PlayStatus::Loading).ok();
                        check_err!(MPV.playlist_previous_weak(), err_tx_2)
                    }
                    VanControl::PauseControl => {
                        let is_pause = MPV.get_property::<bool>("pause").ok();
                        if is_pause == Some(false) {
                            check_err!(MPV.pause(), err_tx_2);
                        } else if is_pause == Some(true) {
                            check_err!(MPV.unpause(), err_tx_2);
                        }
                    }
                    VanControl::Exit => {
                        exit_tx.send(true).unwrap();
                    }
                }
            }
            if get_total_content().ok() == get_current_song_index().ok().map(|x| x + 1) {
                let queue = QUEUE.lock().unwrap();
                let queue_ref = queue
                    .iter()
                    .map(|(x, y, z)| (x.as_str(), *y, *z))
                    .collect::<Vec<_>>();

                check_err!(MPV.playlist_load_files(&queue_ref), err_tx_3);
            }
        });
        scope.spawn(move |_| loop {
            let ev = ev_ctx.wait_event(600.).unwrap_or(Err(Error::Null));
            match ev {
                Ok(Event::PropertyChange {
                    name: "demuxer-cache-state",
                    change: PropertyData::Node(node),
                    ..
                }) => {
                    info!("{:?}", seekable_ranges(node));
                }
                Ok(v) => info!("{:?}", v),
                Err(e) => error!("{}", e),
            }
        });
    })
    .map_err(|e| anyhow!("{:?}", e))?;

    loop {
        if let Ok(e) = err_rx.try_recv() {
            return Err(anyhow!("{}", e));
        }
        if let Ok(exit) = exit_rx.try_recv() {
            if exit {
                return Ok(());
            }
        }
    }
}

pub fn get_playlist() -> Result<Vec<PlayListItem>> {
    let playlist = MPV
        .get_property::<String>("playlist")
        .map_err(|e| anyhow!("{}", e))?;
    let playlist = serde_json::from_str(&playlist)?;

    Ok(playlist)
}

pub fn get_current_media_info() -> Result<MediaInfo> {
    let title = MPV
        .get_property("media-title")
        .map_err(|e| anyhow!("{}", e))?;
    let duration = MPV.get_property("duration").map_err(|e| anyhow!("{}", e))?;
    let artist = MPV
        .get_property::<String>("metadata/by-key/Uploader")
        .map_err(|e| anyhow!("{}", e))?;
    let current_time = MPV.get_property("time-pos").map_err(|e| anyhow!("{}", e))?;

    Ok(MediaInfo {
        title,
        artist,
        duration,
        current_time,
    })
}

#[test]
fn test_play() {
    use std::time::Duration;
    let (getinfo_tx, getinfo_rx) = std::sync::mpsc::channel();
    let (control_tx, control_rx) = std::sync::mpsc::channel();
    add("https://www.bilibili.com/video/BV1NY4y1t7hx").unwrap();
    let work = std::thread::spawn(|| {
        play(control_rx, getinfo_tx).unwrap();
    });
    control_tx.send(VanControl::SetVolume(50.0)).unwrap();
    dbg!(getinfo_rx.recv().unwrap());
    dbg!(get_current_song_index().unwrap());
    control_tx.send(VanControl::NextSong).unwrap();
    dbg!(get_current_song_index().unwrap());
    std::thread::sleep(Duration::from_secs(10));
    control_tx.send(VanControl::NextSong).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    dbg!(get_current_song_index().unwrap());
    control_tx.send(VanControl::PrevSong).unwrap();
    work.join().unwrap();
}
