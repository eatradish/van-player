use anyhow::{anyhow, Result};
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

macro_rules! check_err {
    ($i:expr, $err_tx:expr) => {
        if let Err(e) = $i {
            $err_tx.send(e.to_string()).unwrap();
            error!("{}", e);
        }
    };
}

pub struct Van {
    mpv: Mpv,
    queue: Mutex<Vec<(String, FileState, Option<String>)>>,
}

impl Van {
    pub fn new() -> Result<Self> {
        Ok(Self {
            mpv: Mpv::new().map_err(|e| anyhow!("{}", e))?,
            queue: Mutex::new(Vec::new()),
        })
    }

    pub fn add(&self, url: &str) -> Result<()> {
        let mut queue = self.queue.lock().map_err(|e| anyhow!("{}", e))?;
        queue.push((url.to_string(), FileState::AppendPlay, None));

        Ok(())
    }

    pub fn play(
        &self,
        control_rx: Receiver<VanControl>,
        getinfo_tx: Sender<PlayStatus>,
    ) -> Result<()> {
        self.mpv
            .set_property("volume", DEFAULT_VOL)
            .map_err(|e| anyhow!("{}", e))?;
        let mut ev_ctx = self.mpv.create_event_context();
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
                    self.mpv
                        .set_property("options/ytdl-raw-options", "yes-playlist="),
                    err_tx
                );
                check_err!(self.mpv.set_property("vo", "null"), err_tx);
                let queue = &*self.queue.lock().unwrap();
                let queue = queue
                    .iter()
                    .map(|(x, y, z)| (x.as_str(), *y, z.as_deref()))
                    .collect::<Vec<_>>();
                check_err!(self.mpv.playlist_load_files(&queue), err_tx);
            });
            scope.spawn(move |_| loop {
                // info!("{:?}", MPV.get_property::<String>("playlist"));
                let current_media = self.get_current_media_info();
                if let Ok(m) = current_media {
                    getinfo_tx.send(PlayStatus::MediaInfo(m.clone())).ok();
                    // info!("Send! {:?}", m);
                } else {
                    getinfo_tx.send(PlayStatus::Loading).ok();
                    // info!("Send! Control song");
                }
                if let Ok(v) = control_rx.try_recv() {
                    match v {
                        VanControl::SetVolume(vol) => {
                            check_err!(self.mpv.set_property("volume", vol), err_tx_2)
                        }
                        VanControl::NextSong => {
                            getinfo_tx.send(PlayStatus::Loading).ok();
                            check_err!(self.mpv.playlist_next_weak(), err_tx_2)
                        }
                        VanControl::PrevSong => {
                            getinfo_tx.send(PlayStatus::Loading).ok();
                            check_err!(self.mpv.playlist_previous_weak(), err_tx_2)
                        }
                        VanControl::PauseControl => {
                            let is_pause = self.mpv.get_property::<bool>("pause").ok();
                            if is_pause == Some(false) {
                                check_err!(self.mpv.pause(), err_tx_2);
                            } else if is_pause == Some(true) {
                                check_err!(self.mpv.unpause(), err_tx_2);
                            }
                        }
                        VanControl::Exit => {
                            info!("destroy");
                            check_err!(self.mpv.command("quit", &[]), err_tx_2);
                        }
                    }
                }
                if self.get_total_content().ok()
                    == self.get_current_song_index().ok().map(|x| x + 1)
                {
                    let queue = self.queue.lock().unwrap();
                    let queue_ref = queue
                        .iter()
                        .map(|(x, y, z)| (x.as_str(), *y, z.as_deref()))
                        .collect::<Vec<_>>();

                    check_err!(self.mpv.playlist_load_files(&queue_ref), err_tx_3);
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
        }
    }

    pub fn get_volume(&self) -> Result<f64> {
        self.mpv
            .get_property::<f64>("volume")
            .map_err(|e| anyhow!("{}", e))
    }

    fn get_current_song_index(&self) -> Result<i64> {
        let current = self
            .mpv
            .get_property("playlist-pos")
            .map_err(|e| anyhow!("{}", e))?;

        Ok(current)
    }

    fn get_total_content(&self) -> Result<i64> {
        self.mpv
            .get_property("playlist-count")
            .map_err(|e| anyhow!("{}", e))
    }

    pub fn get_file_name(&self) -> Result<String> {
        self.mpv
            .get_property("filename")
            .map_err(|e| anyhow!("{}", e))
    }

    pub fn force_play(&self, count: i64) -> Result<()> {
        self.mpv
            .set_property("playlist-pos", count)
            .map_err(|e| anyhow!("{}", e))?;

        Ok(())
    }

    pub fn get_playlist(&self) -> Result<Vec<PlayListItem>> {
        let playlist = self
            .mpv
            .get_property::<String>("playlist")
            .map_err(|e| anyhow!("{}", e))?;
        let playlist = serde_json::from_str(&playlist)?;

        Ok(playlist)
    }

    pub fn get_current_media_info(&self) -> Result<MediaInfo> {
        let title = self
            .mpv
            .get_property("media-title")
            .map_err(|e| anyhow!("{}", e))?;
        let duration = self
            .mpv
            .get_property("duration")
            .map_err(|e| anyhow!("{}", e))?;
        let artist = self
            .mpv
            .get_property::<String>("metadata/by-key/Uploader")
            .map_err(|e| anyhow!("{}", e))?;
        let current_time = self
            .mpv
            .get_property("time-pos")
            .map_err(|e| anyhow!("{}", e))?;

        Ok(MediaInfo {
            title,
            artist,
            duration,
            current_time,
        })
    }
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

#[test]
fn test_play() {
    use std::sync::Arc;
    use std::time::Duration;
    let (getinfo_tx, getinfo_rx) = std::sync::mpsc::channel();
    let (control_tx, control_rx) = std::sync::mpsc::channel();
    let van = Mutex::new(Arc::new(Van::new().unwrap()));
    let van_clone = van.lock().unwrap().clone();
    let van = van.lock().unwrap();
    van.add("https://www.bilibili.com/video/BV1NY4y1t7hx")
        .unwrap();
    let work = std::thread::spawn(move || {
        van_clone.play(control_rx, getinfo_tx).unwrap();
    });
    control_tx.send(VanControl::SetVolume(50.0)).unwrap();
    dbg!(getinfo_rx.recv().unwrap());
    dbg!(van.get_current_song_index().unwrap());
    control_tx.send(VanControl::NextSong).unwrap();
    dbg!(van.get_current_song_index().unwrap());
    std::thread::sleep(Duration::from_secs(10));
    control_tx.send(VanControl::NextSong).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    dbg!(van.get_current_song_index().unwrap());
    control_tx.send(VanControl::PrevSong).unwrap();
    work.join().unwrap();
}
