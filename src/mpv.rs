use lazy_static::lazy_static;
use libmpv::{events::*, *};
use std::{collections::HashMap, sync::{Mutex, mpsc::Receiver, atomic::{AtomicUsize, Ordering}}, time::Duration};

const DEFAULT_VOL: f64 = 50.0;

lazy_static! {
    static ref MPV: Mpv = Mpv::new().expect("Can not init mpv");
    static ref QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static ref CURRENT: AtomicUsize = AtomicUsize::new(0);
}

fn add(url: &str) {
    let mut queue = QUEUE.lock().unwrap();
    queue.push(url.to_string());
}

fn play(
    vol_rx: std::sync::mpsc::Receiver<f64>,
    next_rx: std::sync::mpsc::Receiver<bool>,
    prev_rx: std::sync::mpsc::Receiver<bool>,
) -> Result<()> {
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

fn play_inner(
    vol_rx: Receiver<f64>,
    next_rx: Receiver<bool>,
    prev_rx: Receiver<bool>,
) {
    let mut ev_ctx = MPV.create_event_context();
    crossbeam::scope(|scope| {
        scope.spawn(move |_| {
            MPV.set_property("volume", DEFAULT_VOL).unwrap();
            MPV.set_property("vo", "null").unwrap();
            let current = CURRENT.load(Ordering::SeqCst);
            let queue = QUEUE.lock().unwrap();
            let queue = queue.iter().map(|x| x.as_str()).collect::<Vec<_>>();
            let queue = queue
                .into_iter()
                .map(|x| (x, FileState::AppendPlay, None::<&str>))
                .collect::<Vec<_>>();
            MPV.playlist_load_files(&queue[current..]).unwrap();
        });
        scope.spawn(move |_| loop {
            let mut current = CURRENT.load(Ordering::SeqCst);
            let queue = QUEUE.lock().unwrap();
            let queue = queue.iter().map(|x| x.as_str()).collect::<Vec<_>>();
            let queue = queue
                .into_iter()
                .map(|x| (x, FileState::AppendPlay, None::<&str>))
                .collect::<Vec<_>>();
            if let Ok(v) = vol_rx.try_recv() {
                println!("vol is set to {}", v);
                MPV.set_property("volume", v).unwrap();
            }
            if let Ok(next) = next_rx.try_recv() {
                if next {
                    dbg!(current);
                    println!("next!");
                    if current == queue.len() - 1 {
                        CURRENT.store(0, Ordering::SeqCst);
                        MPV.playlist_next_force().unwrap();
                        MPV.playlist_load_files(&queue).unwrap();
                    } else {
                        MPV.playlist_next_force().unwrap();
                        CURRENT.store(current + 1, Ordering::SeqCst);
                    }

                }
            }
            if let Ok(prev) = prev_rx.try_recv() {
                if prev {
                    println!("prev!");
                    if current == 0 {
                        current = queue.len() - 1;
                        CURRENT.store(current, Ordering::SeqCst);
                        MPV.playlist_previous_force().unwrap();
                        MPV.playlist_load_files(&queue[current..]).unwrap();
                    } else {
                        MPV.playlist_previous_force().unwrap();
                        CURRENT.store(current - 1, Ordering::SeqCst);
                    }
                }
            }
        });
        scope.spawn(move |_| loop {
            let ev = ev_ctx.wait_event(600.).unwrap_or(Err(Error::Null));

            match ev {
                Ok(Event::EndFile(r)) => {
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
                Ok(e) => println!("Event triggered: {:?}", e),
                Err(e) => println!("Event errored: {:?}", e),
            }
        });
    })
    .unwrap();
}

#[test]
fn test_play() {
    let (tx, rx) = std::sync::mpsc::channel();
    let (next_tx, next_rx) = std::sync::mpsc::channel();
    let (prev_tx, prev_rx) = std::sync::mpsc::channel();
    tx.send(100.0).unwrap();
    add("https://www.bilibili.com/video/BV1WL4y1F7Uj");
    add("https://www.bilibili.com/video/BV18B4y127QA");
    let work = std::thread::spawn(|| {
        play(rx, next_rx, prev_rx).unwrap();
    });
    tx.send(75.0).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    next_tx.send(true).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    next_tx.send(true).unwrap();

    work.join().unwrap();
}
