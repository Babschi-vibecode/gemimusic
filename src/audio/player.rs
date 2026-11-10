use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Stopped,
    Buffering,
    Playing,
    Paused,
}

#[derive(Debug)]
pub enum AudioCommand {
    PlayUrl(String),
    Pause,
    Resume,
    TogglePlayPause,
    Seek(f32),
    SetVolume(f32),
    Stop,
}

#[derive(Debug, Clone)]
pub struct PlaybackProgress {
    pub position_secs: f32,
    pub duration_secs: f32,
    pub status: PlaybackStatus,
}

pub struct AudioEngine {
    command_tx: Sender<AudioCommand>,
    progress: Arc<Mutex<PlaybackProgress>>,
    is_playing_flag: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (command_tx, command_rx) = bounded::<AudioCommand>(32);
        let progress = Arc::new(Mutex::new(PlaybackProgress {
            position_secs: 0.0,
            duration_secs: 0.0,
            status: PlaybackStatus::Stopped,
        }));
        let is_playing_flag = Arc::new(AtomicBool::new(false));

        let progress_clone = Arc::clone(&progress);
        let flag_clone = Arc::clone(&is_playing_flag);

        // Spawn dedicated audio playback worker thread
        thread::Builder::new()
            .name("gemimusic-audio".to_string())
            .spawn(move || {
                Self::run_audio_loop(command_rx, progress_clone, flag_clone);
            })
            .expect("Failed to spawn audio worker thread");

        Self {
            command_tx,
            progress,
            is_playing_flag,
        }
    }

    pub fn play_url(&self, url: String) {
        let _ = self.command_tx.send(AudioCommand::PlayUrl(url));
    }

    pub fn pause(&self) {
        let _ = self.command_tx.send(AudioCommand::Pause);
    }

    pub fn resume(&self) {
        let _ = self.command_tx.send(AudioCommand::Resume);
    }

    pub fn toggle_play_pause(&self) {
        let _ = self.command_tx.send(AudioCommand::TogglePlayPause);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.command_tx.send(AudioCommand::SetVolume(volume));
    }

    pub fn seek(&self, position_secs: f32) {
        let _ = self.command_tx.send(AudioCommand::Seek(position_secs));
    }

    pub fn stop(&self) {
        let _ = self.command_tx.send(AudioCommand::Stop);
    }

    pub fn get_progress(&self) -> PlaybackProgress {
        self.progress.lock().clone()
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing_flag.load(Ordering::Relaxed)
    }

    /// Background audio playback loop
    fn run_audio_loop(
        rx: Receiver<AudioCommand>,
        progress: Arc<Mutex<PlaybackProgress>>,
        is_playing_flag: Arc<AtomicBool>,
    ) {
        // Attempt to open default audio output stream (cpal / AAudio / WASAPI / ALSA)
        let (_stream, stream_handle) = match OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(e) => {
                log::error!("Audio output device initialization failed: {}", e);
                (None, None)
            }
        };

        let mut sink: Option<Sink> = stream_handle.as_ref().and_then(|h| Sink::try_new(h).ok());
        let mut current_volume: f32 = 0.8;
        let mut last_tick = Instant::now();
        let mut track_start_time: Option<Instant> = None;
        let mut elapsed_before_pause: Duration = Duration::ZERO;

        loop {
            // Check for incoming playback commands (with 50ms timeout for smooth position ticking)
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(cmd) => match cmd {
                    AudioCommand::PlayUrl(url) => {
                        log::info!("Starting stream download & playback for: {}", url);
                        {
                            let mut p = progress.lock();
                            p.status = PlaybackStatus::Buffering;
                            p.position_secs = 0.0;
                        }
                        is_playing_flag.store(false, Ordering::Relaxed);

                        // If previous sink is active, stop it
                        if let Some(s) = &sink {
                            s.stop();
                        }

                        // Recreate sink
                        if let Some(h) = &stream_handle {
                            sink = Sink::try_new(h).ok();
                        }

                        if let Some(ref s) = sink {
                            s.set_volume(current_volume);

                            // Fetch audio bytes over HTTP (lightweight buffer)
                            let client = reqwest::blocking::Client::builder()
                                .timeout(Duration::from_secs(15))
                                .build();

                            match client {
                                Ok(c) => match c.get(&url).send() {
                                    Ok(resp) => match resp.bytes() {
                                        Ok(bytes) => {
                                            let cursor = Cursor::new(bytes.to_vec());
                                            match Decoder::new(cursor) {
                                                Ok(source) => {
                                                    s.append(source);
                                                    s.play();
                                                    track_start_time = Some(Instant::now());
                                                    elapsed_before_pause = Duration::ZERO;

                                                    let mut p = progress.lock();
                                                    p.status = PlaybackStatus::Playing;
                                                    is_playing_flag.store(true, Ordering::Relaxed);
                                                    log::info!("Audio playback successfully started");
                                                }
                                                Err(e) => {
                                                    log::error!("Decoder error: {}", e);
                                                    let mut p = progress.lock();
                                                    p.status = PlaybackStatus::Stopped;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            log::error!("Failed reading audio bytes: {}", e);
                                            let mut p = progress.lock();
                                            p.status = PlaybackStatus::Stopped;
                                        }
                                    },
                                    Err(e) => {
                                        log::error!("HTTP request error for audio stream: {}", e);
                                        let mut p = progress.lock();
                                        p.status = PlaybackStatus::Stopped;
                                    }
                                },
                                Err(e) => {
                                    log::error!("Client builder error: {}", e);
                                }
                            }
                        }
                    }
                    AudioCommand::Pause => {
                        if let Some(s) = &sink {
                            if !s.is_paused() {
                                s.pause();
                                if let Some(start) = track_start_time {
                                    elapsed_before_pause += start.elapsed();
                                }
                                track_start_time = None;
                                let mut p = progress.lock();
                                p.status = PlaybackStatus::Paused;
                                is_playing_flag.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                    AudioCommand::Resume => {
                        if let Some(s) = &sink {
                            if s.is_paused() {
                                s.play();
                                track_start_time = Some(Instant::now());
                                let mut p = progress.lock();
                                p.status = PlaybackStatus::Playing;
                                is_playing_flag.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    AudioCommand::TogglePlayPause => {
                        if let Some(s) = &sink {
                            if s.is_paused() {
                                s.play();
                                track_start_time = Some(Instant::now());
                                let mut p = progress.lock();
                                p.status = PlaybackStatus::Playing;
                                is_playing_flag.store(true, Ordering::Relaxed);
                            } else if !s.empty() {
                                s.pause();
                                if let Some(start) = track_start_time {
                                    elapsed_before_pause += start.elapsed();
                                }
                                track_start_time = None;
                                let mut p = progress.lock();
                                p.status = PlaybackStatus::Paused;
                                is_playing_flag.store(false, Ordering::Relaxed);
                            }
                        }
                    }
                    AudioCommand::SetVolume(vol) => {
                        current_volume = vol.clamp(0.0, 1.0);
                        if let Some(s) = &sink {
                            s.set_volume(current_volume);
                        }
                    }
                    AudioCommand::Seek(pos) => {
                        if let Some(s) = &sink {
                            let seek_dur = Duration::from_secs_f32(pos);
                            let _ = s.try_seek(seek_dur);
                            elapsed_before_pause = seek_dur;
                            if track_start_time.is_some() {
                                track_start_time = Some(Instant::now());
                            }
                            let mut p = progress.lock();
                            p.position_secs = pos;
                        }
                    }
                    AudioCommand::Stop => {
                        if let Some(s) = &sink {
                            s.stop();
                        }
                        track_start_time = None;
                        elapsed_before_pause = Duration::ZERO;
                        let mut p = progress.lock();
                        p.status = PlaybackStatus::Stopped;
                        p.position_secs = 0.0;
                        is_playing_flag.store(false, Ordering::Relaxed);
                    }
                },
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    // Periodic update of playback position
                    if let Some(s) = &sink {
                        if !s.is_paused() && !s.empty() {
                            let current_dur = elapsed_before_pause
                                + track_start_time.map_or(Duration::ZERO, |t| t.elapsed());
                            let mut p = progress.lock();
                            p.position_secs = current_dur.as_secs_f32();
                            p.status = PlaybackStatus::Playing;
                            is_playing_flag.store(true, Ordering::Relaxed);
                        } else if s.empty() && track_start_time.is_some() {
                            // Song completed
                            let mut p = progress.lock();
                            p.status = PlaybackStatus::Stopped;
                            is_playing_flag.store(false, Ordering::Relaxed);
                            track_start_time = None;
                            elapsed_before_pause = Duration::ZERO;
                        }
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    // Application exiting
                    break;
                }
            }
        }
    }
}
