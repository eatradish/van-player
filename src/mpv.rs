use cursive::views::TextContent;
use lazy_static::lazy_static;
use libmpv::{events::*, *};
use log::{error, info, warn};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, Sender},
        Mutex, Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, bail, Result};

#[derive(Debug)]
pub struct MediaInfo {
    pub title: String,
    pub duration: f64,
    pub current_time: f64,
}

pub const DEFAULT_VOL: f64 = 50.0;

lazy_static! {
    pub static ref MPV: Mpv = Mpv::new().expect("Can not init mpv");
    pub static ref QUEUE: Mutex<Vec<(&'static str, FileState, Option<&'static str>)>> =
        Mutex::new(Vec::new());
    pub static ref CURRENT: AtomicUsize = AtomicUsize::new(0);
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

pub fn add(url: &'static str) -> Result<()> {
    let mut queue = QUEUE.lock().map_err(|e| anyhow!("{}", e))?;
    queue.push((url, FileState::AppendPlay, None));

    Ok(())
}

pub fn play(
    vol_rx: Receiver<f64>,
    next_rx: Receiver<bool>,
    prev_rx: Receiver<bool>,
    getinfo_tx: Sender<MediaInfo>,
) -> Result<()> {
    MPV.set_property("volume", DEFAULT_VOL)
        .map_err(|e| anyhow!("{}", e))?;
    play_inner(vol_rx, next_rx, prev_rx, getinfo_tx)?;

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

fn play_inner(
    vol_rx: Receiver<f64>,
    next_rx: Receiver<bool>,
    prev_rx: Receiver<bool>,
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
            let current = CURRENT.load(Ordering::SeqCst);
            check_err!(
                MPV.playlist_load_files(&QUEUE.lock().unwrap()[current..]),
                err_tx
            );
        });
        scope.spawn(move |_| loop {
            let mut current = CURRENT.load(Ordering::SeqCst);
            if let Ok(v) = vol_rx.try_recv() {
                check_err!(MPV.set_property("volume", v), err_tx_2);
            }
            if let Ok(next) = next_rx.try_recv() {
                let queue = QUEUE.lock();
                check_err!(queue, err_tx_2);
                let queue = queue.unwrap();
                if next {
                    if current == queue.len() - 1 {
                        check_err!(MPV.playlist_next_force(), err_tx_2);
                        check_err!(MPV.playlist_load_files(&queue[current..]), err_tx_2);
                        continue;
                    } else {
                        check_err!(MPV.playlist_next_force(), err_tx_2);
                    }
                }
            }
            if let Ok(prev) = prev_rx.try_recv() {
                let queue = QUEUE.lock();
                check_err!(queue, err_tx_2);
                let queue = queue.unwrap();
                if prev {
                    if current == 0 {
                        current = queue.len() - 1;
                        check_err!(MPV.playlist_previous_force(), err_tx_2);
                        check_err!(MPV.playlist_load_files(&queue[current..]), err_tx_2);
                        continue;
                    } else {
                        check_err!(MPV.playlist_previous_force(), err_tx_2);
                    }
                }
            }
        });
        scope.spawn(move |_| loop {
            let ev = ev_ctx.wait_event(600.).unwrap_or(Err(Error::Null));
            match ev {
                Ok(Event::EndFile(_)) => {
                    let current = CURRENT.load(Ordering::SeqCst);
                    let queue = QUEUE.lock();
                    check_err!(queue, err_tx_3);
                    let queue = queue.unwrap();
                    if current < queue.len() - 1 {
                        CURRENT.store(current + 1, Ordering::SeqCst);
                    }
                }
                Ok(Event::PropertyChange {
                    name: "demuxer-cache-state",
                    change: PropertyData::Node(node),
                    ..
                }) => {
                    let current_media = get_current_media_info();
                    if let Ok(m) = current_media {
                        info!("Send!, {:?}", m);
                        getinfo_tx.send(m).ok();
                    }
                    info!("{:?}", seekable_ranges(node));
                }
                Ok(Event::Deprecated(_)) => {
                    let queue = QUEUE.lock();
                    check_err!(queue, err_tx_3);
                    let queue = queue.unwrap();
                    check_err!(MPV.playlist_load_files(&queue), err_tx_3);
                    CURRENT.store(0, Ordering::SeqCst);
                }
                Ok(e) => info!("Event triggered: {:?}", e),
                Err(e) => error!("Event errored: {:?}", e),
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
        .get_property::<f64>("duration")
        .map_err(|e| anyhow!("{}", e))?;
    let current_time = MPV
        .get_property::<f64>("time-pos")
        .map_err(|e| anyhow!("{}", e))?;

    return Ok(MediaInfo {
        title,
        duration,
        current_time,
    });
}

#[test]
fn test_play() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (next_tx, next_rx) = std::sync::mpsc::channel();
    let (prev_tx, prev_rx) = std::sync::mpsc::channel();
    let (getinfo_tx, getinfo_rx) = std::sync::mpsc::channel();
    add("https://www.bilibili.com/video/BV1gr4y1b7vN").unwrap();
    add("https://www.bilibili.com/video/BV1GR4y1w7CR").unwrap();
    add("https://www.bilibili.com/video/BV133411V7dY").unwrap();
    add("https://www.bilibili.com/video/BV1WL4y1F7Uj").unwrap();
    add("https://www.bilibili.com/video/BV18B4y127QA").unwrap();
    add("https://www.bilibili.com/video/BV1NY4y1t7hx?p=7").unwrap();
    let work = std::thread::spawn(|| {
        play(rx, next_rx, prev_rx, getinfo_tx).unwrap();
    });
    tx.send(100.0).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    std::thread::sleep(Duration::from_secs(10));
    next_tx.send(true).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    next_tx.send(true).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    prev_tx.send(true).unwrap();
    work.join().unwrap();
}
