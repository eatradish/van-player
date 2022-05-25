use lazy_static::lazy_static;
use libmpv::{events::*, *};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc::Receiver,
        Mutex,
    },
};

const DEFAULT_VOL: f64 = 50.0;

lazy_static! {
    static ref MPV: Mpv = Mpv::new().expect("Can not init mpv");
    static ref QUEUE: Mutex<Vec<(&'static str, FileState, Option<&'static str>)>> =
        Mutex::new(Vec::new());
    static ref CURRENT: AtomicUsize = AtomicUsize::new(0);
}

fn add(url: &'static str) {
    let mut queue = QUEUE.lock().unwrap();
    queue.push((url, FileState::AppendPlay, None));
}

fn play(vol_rx: Receiver<f64>, next_rx: Receiver<bool>, prev_rx: Receiver<bool>) -> Result<()> {
    play_inner(vol_rx, next_rx, prev_rx);

    Ok(())
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

fn play_inner(vol_rx: Receiver<f64>, next_rx: Receiver<bool>, prev_rx: Receiver<bool>) {
    let mut ev_ctx = MPV.create_event_context();
    ev_ctx.observe_property("volume", Format::Int64, 0).unwrap();
    ev_ctx
        .observe_property("demuxer-cache-state", Format::Node, 0)
        .unwrap();
    crossbeam::scope(|scope| {
        scope.spawn(move |_| {
            MPV.set_property("volume", DEFAULT_VOL).unwrap();
            MPV.set_property("vo", "null").unwrap();
            let current = CURRENT.load(Ordering::SeqCst);
            MPV.playlist_load_files(&QUEUE.lock().unwrap()[current..])
                .unwrap();
        });
        scope.spawn(move |_| loop {
            let mut current = CURRENT.load(Ordering::SeqCst);
            if let Ok(v) = vol_rx.try_recv() {
                println!("vol is set to {}", v);
                MPV.set_property("volume", v).unwrap();
            }
            if let Ok(next) = next_rx.try_recv() {
                let queue = QUEUE.lock().unwrap();
                if next {
                    if current == queue.len() - 1 {
                        MPV.playlist_next_force().unwrap();
                        MPV.playlist_load_files(&queue[current..]).unwrap();
                        continue;
                    } else {
                        MPV.playlist_next_force().unwrap();
                    }
                }
            }
            if let Ok(prev) = prev_rx.try_recv() {
                let queue = QUEUE.lock().unwrap();
                if prev {
                    println!("prev!");
                    if current == 0 {
                        current = queue.len() - 1;
                        MPV.playlist_previous_force().unwrap();
                        MPV.playlist_load_files(&queue[current..]).unwrap();
                        continue;
                    } else {
                        MPV.playlist_previous_force().unwrap();
                    }
                }
            }
        });
        scope.spawn(move |_| loop {
            let ev = ev_ctx.wait_event(600.).unwrap_or(Err(Error::Null));

            match ev {
                Ok(Event::EndFile(r)) => {
                    println!("next!");
                    let current = CURRENT.load(Ordering::SeqCst);
                    let queue_len = QUEUE.lock().unwrap().len();
                    if current < queue_len - 1 {
                        CURRENT.store(current + 1, Ordering::SeqCst);
                    }
                    println!("Exiting! Reason: {:?}", r);
                    break;
                }

                Ok(Event::PropertyChange {
                    name: "demuxer-cache-state",
                    change: PropertyData::Node(mpv_node),
                    ..
                }) => {
                    let ranges = seekable_ranges(mpv_node).unwrap();
                    println!("Seekable ranges updated: {:?}", ranges);
                }
                Ok(Event::Deprecated(_)) => {
                    let queue = QUEUE.lock().unwrap();
                    MPV.playlist_load_files(&queue).unwrap();
                    CURRENT.store(0, Ordering::SeqCst);
                }
                Ok(e) => println!("Event triggered: {:?}", e),
                Err(e) => println!("Event errored: {:?}", e),
            }
        });
    })
    .unwrap();
}

#[test]
fn test_play() {
    use std::time::Duration;
    let (tx, rx) = std::sync::mpsc::channel();
    let (next_tx, next_rx) = std::sync::mpsc::channel();
    let (prev_tx, prev_rx) = std::sync::mpsc::channel();
    add("https://www.bilibili.com/video/BV1GR4y1w7CR");
    add("https://www.bilibili.com/video/BV133411V7dY");
    add("https://www.bilibili.com/video/BV1WL4y1F7Uj");
    add("https://www.bilibili.com/video/BV18B4y127QA");
    let work = std::thread::spawn(|| {
        play(rx, next_rx, prev_rx).unwrap();
    });
    tx.send(100.0).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    next_tx.send(true).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    next_tx.send(true).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    prev_tx.send(true).unwrap();
    work.join().unwrap();
}
