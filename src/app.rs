use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::api::models::{Playlist, Track};
use crate::api::{GoogleOAuthClient, InnerTubeClient};
use crate::audio::{AudioEngine, PlaybackStatus};
use crate::config::ConfigManager;

// Slint compiled definitions
slint::include_modules!();

pub struct AppState {
    pub inner_tube: Arc<InnerTubeClient>,
    pub oauth: Arc<GoogleOAuthClient>,
    pub audio: Arc<AudioEngine>,
    pub config_manager: Arc<ConfigManager>,
    pub queue: Arc<Mutex<Vec<Track>>>,
    pub current_track_idx: Arc<Mutex<Option<usize>>>,
    pub current_playlists: Arc<Mutex<Vec<Playlist>>>,
    pub is_auth_polling: Arc<AtomicBool>,
}

pub struct App {
    pub ui: MainWindow,
    pub state: Arc<AppState>,
    pub tokio_handle: tokio::runtime::Handle,
}

impl App {
    pub fn new(tokio_handle: tokio::runtime::Handle) -> Self {
        let ui = MainWindow::new().expect("Failed to create MainWindow");
        let config_manager = Arc::new(ConfigManager::new());
        let saved_config = config_manager.load();

        let state = Arc::new(AppState {
            inner_tube: Arc::new(InnerTubeClient::new()),
            oauth: Arc::new(GoogleOAuthClient::default()),
            audio: Arc::new(AudioEngine::new()),
            config_manager,
            queue: Arc::new(Mutex::new(Vec::new())),
            current_track_idx: Arc::new(Mutex::new(None)),
            current_playlists: Arc::new(Mutex::new(Vec::new())),
            is_auth_polling: Arc::new(AtomicBool::new(false)),
        });

        // Initialize saved state in UI
        if let Some(tokens) = &saved_config.auth_tokens {
            if !tokens.is_expired() {
                ui.set_is_logged_in(true);
                if let Some(email) = &saved_config.user_email {
                    ui.set_user_email(SharedString::from(email.as_str()));
                }
                if let Some(name) = &saved_config.user_name {
                    ui.set_user_display_name(SharedString::from(name.as_str()));
                }
            }
        }
        ui.set_volume(saved_config.volume);

        Self {
            ui,
            state,
            tokio_handle,
        }
    }

    pub fn setup_callbacks(&self) {
        let ui_weak = self.ui.as_weak();
        let state = Arc::clone(&self.state);
        let rt = self.tokio_handle.clone();

        // 1. Search submitted callback
        let state_search = Arc::clone(&state);
        let rt_search = rt.clone();
        let ui_search = ui_weak.clone();
        self.ui.on_search_submitted(move |query| {
            let q = query.to_string();
            if q.trim().is_empty() {
                return;
            }

            let state_c = Arc::clone(&state_search);
            let ui_c = ui_search.clone();
            rt_search.spawn(async move {
                log::info!("Searching YouTube Music for query: {}", q);
                match state_c.inner_tube.search_tracks(&q).await {
                    Ok(tracks) => {
                        log::info!("Found {} tracks", tracks.len());
                        *state_c.queue.lock() = tracks.clone();

                        let mut slint_tracks = Vec::new();
                        for t in &tracks {
                            slint_tracks.push(TrackItemData {
                                id: SharedString::from(t.id.as_str()),
                                title: SharedString::from(t.title.as_str()),
                                artist: SharedString::from(t.artist.as_str()),
                                album: SharedString::from(t.album.as_str()),
                                duration: SharedString::from(t.duration_text.as_str()),
                                thumbnail_url: SharedString::from(t.thumbnail_url.as_str()),
                            });
                        }

                        let model = Rc::new(VecModel::from(slint_tracks));
                        let _ = ui_c.upgrade_in_event_loop(move |ui| {
                            ui.set_search_results(ModelRc::from(model));
                        });
                    }
                    Err(e) => {
                        log::error!("Search error: {}", e);
                    }
                }
            });
        });

        // 2. Track Play Requested (from Search list or Playlist)
        let state_play = Arc::clone(&state);
        let rt_play = rt.clone();
        let ui_play = ui_weak.clone();
        self.ui.on_track_play_requested(move |index| {
            let idx = index as usize;
            let current_queue = state_play.queue.lock().clone();

            if idx >= current_queue.len() {
                return;
            }

            let track = current_queue[idx].clone();
            *state_play.current_track_idx.lock() = Some(idx);

            // Update UI Now Playing state immediately
            let title_s = SharedString::from(track.title.as_str());
            let artist_s = SharedString::from(track.artist.as_str());
            let total_dur_s = SharedString::from(
                if !track.duration_text.is_empty() {
                    track.duration_text.as_str()
                } else {
                    "3:30"
                },
            );

            let _ = ui_play.upgrade_in_event_loop(move |ui| {
                ui.set_now_playing_title(title_s);
                ui.set_now_playing_artist(artist_s);
                ui.set_time_total_str(total_dur_s);
                ui.set_is_buffering(true);
            });

            // Fetch audio stream URL in background and send to audio engine
            let state_c = Arc::clone(&state_play);
            let ui_c = ui_play.clone();
            rt_play.spawn(async move {
                log::info!("Fetching stream URL for track: {} ({})", track.title, track.id);
                match state_c.inner_tube.get_audio_stream_url(&track.id).await {
                    Ok(stream_url) => {
                        log::info!("Audio stream acquired, playing...");
                        state_c.audio.play_url(stream_url);
                        let _ = ui_c.upgrade_in_event_loop(|ui| {
                            ui.set_is_buffering(false);
                            ui.set_is_playing(true);
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to obtain audio stream: {}", e);
                        let _ = ui_c.upgrade_in_event_loop(|ui| {
                            ui.set_is_buffering(false);
                            ui.set_now_playing_title(SharedString::from("Playback Failed"));
                            ui.set_now_playing_artist(SharedString::from("Stream unavailable for this track"));
                        });
                    }
                }
            });
        });

        // 3. Play / Pause Toggle
        let audio_toggle = Arc::clone(&self.state.audio);
        let ui_toggle = ui_weak.clone();
        self.ui.on_toggle_play_pause(move || {
            audio_toggle.toggle_play_pause();
            let is_now_playing = audio_toggle.is_playing();
            let _ = ui_toggle.upgrade_in_event_loop(move |ui| {
                ui.set_is_playing(is_now_playing);
            });
        });

        // 4. Next / Prev Track Controls
        let state_next = Arc::clone(&state);
        let rt_next = rt.clone();
        let ui_next = ui_weak.clone();
        self.ui.on_play_next(move || {
            let q_len = state_next.queue.lock().len();
            if q_len == 0 {
                return;
            }
            let next_idx = {
                let mut cur = state_next.current_track_idx.lock();
                let next = cur.map_or(0, |i| (i + 1) % q_len);
                *cur = Some(next);
                next
            };
            Self::trigger_play_track_at_index(&state_next, &rt_next, &ui_next, next_idx);
        });

        let state_prev = Arc::clone(&state);
        let rt_prev = rt.clone();
        let ui_prev = ui_weak.clone();
        self.ui.on_play_prev(move || {
            let q_len = state_prev.queue.lock().len();
            if q_len == 0 {
                return;
            }
            let prev_idx = {
                let mut cur = state_prev.current_track_idx.lock();
                let prev = cur.map_or(0, |i| if i == 0 { q_len - 1 } else { i - 1 });
                *cur = Some(prev);
                prev
            };
            Self::trigger_play_track_at_index(&state_prev, &rt_prev, &ui_prev, prev_idx);
        });

        // 5. Seek & Volume
        let audio_seek = Arc::clone(&self.state.audio);
        self.ui.on_seek_requested(move |percent| {
            let progress = audio_seek.get_progress();
            if progress.duration_secs > 0.0 {
                let target_pos = progress.duration_secs * percent.clamp(0.0, 1.0);
                audio_seek.seek(target_pos);
            }
        });

        let audio_vol = Arc::clone(&self.state.audio);
        self.ui.on_volume_changed(move |vol| {
            audio_vol.set_volume(vol);
        });

        // 6. Google Account Login (Device Flow RFC 8628)
        let state_auth = Arc::clone(&state);
        let rt_auth = rt.clone();
        let ui_auth = ui_weak.clone();
        self.ui.on_start_google_login(move || {
            let state_c = Arc::clone(&state_auth);
            let ui_c = ui_auth.clone();
            let rt_c = rt_auth.clone();

            rt_auth.spawn(async move {
                log::info!("Requesting Google OAuth device code...");
                match state_c.oauth.request_device_code().await {
                    Ok(dev_resp) => {
                        log::info!("Device code received: {}", dev_resp.user_code);
                        let user_code = SharedString::from(dev_resp.user_code.as_str());
                        let verif_url = SharedString::from(dev_resp.verification_url.as_str());

                        let _ = ui_c.upgrade_in_event_loop(move |ui| {
                            ui.set_auth_device_code(user_code);
                            ui.set_auth_verification_url(verif_url);
                            ui.set_auth_status_text(SharedString::from("Waiting for confirmation on your device..."));
                            ui.set_show_auth_modal(true);
                        });

                        // Start background polling loop
                        state_c.is_auth_polling.store(true, Ordering::SeqCst);
                        let poll_state = Arc::clone(&state_c);
                        let poll_ui = ui_c.clone();
                        let device_code = dev_resp.device_code;
                        let interval = Duration::from_secs(dev_resp.interval.max(3));

                        rt_c.spawn(async move {
                            while poll_state.is_auth_polling.load(Ordering::SeqCst) {
                                tokio::time::sleep(interval).await;
                                if !poll_state.is_auth_polling.load(Ordering::SeqCst) {
                                    break;
                                }

                                match poll_state.oauth.poll_token(&device_code).await {
                                    Ok(Some(tokens)) => {
                                        log::info!("Google OAuth Login Successful!");
                                        poll_state.is_auth_polling.store(false, Ordering::SeqCst);

                                        // Fetch User profile
                                        let mut profile_name = "Connected User".to_string();
                                        let mut profile_email = "user@gmail.com".to_string();

                                        if let Ok(profile) = poll_state.oauth.get_user_profile(&tokens.access_token).await {
                                            profile_name = profile.name;
                                            profile_email = profile.email;
                                        }

                                        // Save tokens locally
                                        poll_state.config_manager.save_tokens(
                                            tokens.clone(),
                                            Some(profile_email.clone()),
                                            Some(profile_name.clone()),
                                        );

                                        let name_s = SharedString::from(profile_name.as_str());
                                        let email_s = SharedString::from(profile_email.as_str());

                                        let _ = poll_ui.upgrade_in_event_loop(move |ui| {
                                            ui.set_show_auth_modal(false);
                                            ui.set_is_logged_in(true);
                                            ui.set_user_display_name(name_s);
                                            ui.set_user_email(email_s);
                                        });

                                        // Automatically load synced playlists
                                        Self::sync_playlists_internal(&poll_state, &poll_ui, &tokens.access_token).await;
                                        break;
                                    }
                                    Ok(None) => {
                                        // Still waiting, continue polling
                                    }
                                    Err(e) => {
                                        log::warn!("OAuth polling status: {}", e);
                                        let err_msg = SharedString::from(format!("{}", e));
                                        let _ = poll_ui.upgrade_in_event_loop(move |ui| {
                                            ui.set_auth_status_text(err_msg);
                                        });
                                        poll_state.is_auth_polling.store(false, Ordering::SeqCst);
                                        break;
                                    }
                                }
                            }
                        });
                    }
                    Err(e) => {
                        log::error!("Failed to request device code: {}", e);
                    }
                }
            });
        });

        // 7. Cancel Google Login
        let state_cancel = Arc::clone(&state);
        let ui_cancel = ui_weak.clone();
        self.ui.on_cancel_google_login(move || {
            state_cancel.is_auth_polling.store(false, Ordering::SeqCst);
            let _ = ui_cancel.upgrade_in_event_loop(|ui| {
                ui.set_show_auth_modal(false);
            });
        });

        // 8. Sync Playlists
        let state_sync = Arc::clone(&state);
        let rt_sync = rt.clone();
        let ui_sync = ui_weak.clone();
        self.ui.on_sync_playlists(move || {
            let config = state_sync.config_manager.load();
            if let Some(tokens) = config.auth_tokens {
                let state_c = Arc::clone(&state_sync);
                let ui_c = ui_sync.clone();
                rt_sync.spawn(async move {
                    Self::sync_playlists_internal(&state_c, &ui_c, &tokens.access_token).await;
                });
            }
        });

        // 9. Open Playlist
        let state_pl_open = Arc::clone(&state);
        let rt_pl_open = rt.clone();
        let ui_pl_open = ui_weak.clone();
        self.ui.on_playlist_opened(move |idx| {
            let index = idx as usize;
            let playlists = state_pl_open.current_playlists.lock().clone();
            if index >= playlists.len() {
                return;
            }

            let pl = playlists[index].clone();
            let title_s = SharedString::from(pl.title.as_str());
            let ui_c = ui_pl_open.clone();
            let state_c = Arc::clone(&state_pl_open);

            let _ = ui_c.upgrade_in_event_loop(move |ui| {
                ui.set_selected_playlist_title(title_s);
            });

            let config = state_c.config_manager.load();
            let token = config.auth_tokens.map(|t| t.access_token);

            rt_pl_open.spawn(async move {
                log::info!("Fetching tracks for playlist: {}", pl.id);
                if let Ok(tracks) = state_c.inner_tube.fetch_playlist_tracks(&pl.id, token.as_deref()).await {
                    *state_c.queue.lock() = tracks.clone();

                    let mut slint_tracks = Vec::new();
                    for t in &tracks {
                        slint_tracks.push(TrackItemData {
                            id: SharedString::from(t.id.as_str()),
                            title: SharedString::from(t.title.as_str()),
                            artist: SharedString::from(t.artist.as_str()),
                            album: SharedString::from(t.album.as_str()),
                            duration: SharedString::from(t.duration_text.as_str()),
                            thumbnail_url: SharedString::from(t.thumbnail_url.as_str()),
                        });
                    }

                    let model = Rc::new(VecModel::from(slint_tracks));
                    let _ = ui_c.upgrade_in_event_loop(move |ui| {
                        ui.set_current_playlist_tracks(ModelRc::from(model));
                    });
                }
            });
        });

        // 10. Back to Playlists List
        let ui_back = ui_weak.clone();
        self.ui.on_back_to_playlists(move || {
            let _ = ui_back.upgrade_in_event_loop(|ui| {
                ui.set_selected_playlist_title(SharedString::from(""));
            });
        });

        // 11. Sign Out
        let state_signout = Arc::clone(&state);
        let ui_signout = ui_weak.clone();
        self.ui.on_sign_out(move || {
            state_signout.config_manager.clear_auth();
            let _ = ui_signout.upgrade_in_event_loop(|ui| {
                ui.set_is_logged_in(false);
                ui.set_user_display_name(SharedString::from(""));
                ui.set_user_email(SharedString::from(""));
                ui.set_user_playlists(ModelRc::from(Rc::new(VecModel::default())));
            });
        });
    }

    /// Periodic playback ticker (progress bar, duration, elapsed time)
    pub fn start_playback_timer(&self) -> slint::Timer {
        let timer = slint::Timer::default();
        let ui_weak = self.ui.as_weak();
        let audio = Arc::clone(&self.state.audio);

        timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            let progress = audio.get_progress();
            let ui_weak_c = ui_weak.clone();

            let _ = ui_weak_c.upgrade_in_event_loop(move |ui| {
                let is_playing = progress.status == PlaybackStatus::Playing;
                let is_buffering = progress.status == PlaybackStatus::Buffering;

                ui.set_is_playing(is_playing);
                ui.set_is_buffering(is_buffering);

                let pos = progress.position_secs;
                let dur = progress.duration_secs.max(1.0);
                ui.set_progress_percent(pos / dur);

                let elapsed_min = (pos as u32) / 60;
                let elapsed_sec = (pos as u32) % 60;
                ui.set_time_elapsed_str(SharedString::from(format!("{}:{:02}", elapsed_min, elapsed_sec)));
            });
        });

        timer
    }

    fn trigger_play_track_at_index(
        state: &Arc<AppState>,
        rt: &tokio::runtime::Handle,
        ui: &slint::Weak<MainWindow>,
        idx: usize,
    ) {
        let queue = state.queue.lock().clone();
        if idx >= queue.len() {
            return;
        }
        let track = queue[idx].clone();
        let title_s = SharedString::from(track.title.as_str());
        let artist_s = SharedString::from(track.artist.as_str());
        let dur_s = SharedString::from(
            if !track.duration_text.is_empty() {
                track.duration_text.as_str()
            } else {
                "3:30"
            },
        );

        let ui_c = ui.clone();
        let _ = ui_c.upgrade_in_event_loop(move |ui| {
            ui.set_now_playing_title(title_s);
            ui.set_now_playing_artist(artist_s);
            ui.set_time_total_str(dur_s);
            ui.set_is_buffering(true);
        });

        let state_c = Arc::clone(state);
        let ui_c2 = ui.clone();
        rt.spawn(async move {
            if let Ok(stream_url) = state_c.inner_tube.get_audio_stream_url(&track.id).await {
                state_c.audio.play_url(stream_url);
                let _ = ui_c2.upgrade_in_event_loop(|ui| {
                    ui.set_is_buffering(false);
                    ui.set_is_playing(true);
                });
            }
        });
    }

    async fn sync_playlists_internal(
        state: &Arc<AppState>,
        ui: &slint::Weak<MainWindow>,
        token: &str,
    ) {
        log::info!("Syncing YouTube Music user playlists...");
        match state.inner_tube.fetch_user_playlists(token).await {
            Ok(playlists) => {
                log::info!("Fetched {} synced playlists", playlists.len());
                *state.current_playlists.lock() = playlists.clone();

                let mut slint_pls = Vec::new();
                for pl in &playlists {
                    slint_pls.push(PlaylistItemData {
                        id: SharedString::from(pl.id.as_str()),
                        title: SharedString::from(pl.title.as_str()),
                        item_count: pl.track_count as i32,
                        thumbnail_url: SharedString::from(pl.thumbnail_url.as_str()),
                    });
                }

                let model = Rc::new(VecModel::from(slint_pls));
                let _ = ui.upgrade_in_event_loop(move |ui| {
                    ui.set_user_playlists(ModelRc::from(model));
                });
            }
            Err(e) => {
                log::error!("Error syncing playlists: {}", e);
            }
        }
    }
}
