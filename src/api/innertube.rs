use super::models::{Playlist, StreamFormat, Track};
use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE, USER_AGENT};
use serde_json::{json, Value};
use std::time::Duration;

pub struct InnerTubeClient {
    client: reqwest::Client,
    base_url: &'static str,
}

impl Default for InnerTubeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl InnerTubeClient {
    pub fn new() -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            ),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-youtube-client-name",
            HeaderValue::from_static("67"), // WEB_REMIX
        );
        headers.insert(
            "x-youtube-client-version",
            HeaderValue::from_static("1.20231211.01.00"),
        );
        headers.insert(
            "x-origin",
            HeaderValue::from_static("https://music.youtube.com"),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to initialize reqwest client");

        Self {
            client,
            base_url: "https://music.youtube.com/youtubei/v1",
        }
    }

    /// Search for songs and tracks on YouTube Music
    pub async fn search_tracks(&self, query: &str) -> Result<Vec<Track>, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!("{}/search", self.base_url);
        let payload = json!({
            "context": {
                "client": {
                    "clientName": "WEB_REMIX",
                    "clientVersion": "1.20231211.01.00",
                    "hl": "en",
                    "gl": "US"
                }
            },
            "query": query,
            // Filter to Songs / Tracks only:
            "params": "Eg-KAQwIABAAGAAgACgAMABqChAEEAMQCRAFEAo%3D"
        });

        let res = self.client.post(&url).json(&payload).send().await?;
        let json_body: Value = res.json().await?;

        let mut tracks = Vec::new();
        self.parse_search_results(&json_body, &mut tracks);

        // Fallback search parsing if filter yielded empty
        if tracks.is_empty() {
            let payload_unfiltered = json!({
                "context": {
                    "client": {
                        "clientName": "WEB_REMIX",
                        "clientVersion": "1.20231211.01.00",
                        "hl": "en",
                        "gl": "US"
                    }
                },
                "query": query
            });
            let res2 = self.client.post(&url).json(&payload_unfiltered).send().await?;
            let json_body2: Value = res2.json().await?;
            self.parse_search_results(&json_body2, &mut tracks);
        }

        Ok(tracks)
    }

    /// Helper to recursively extract track items from YouTube Music JSON response
    fn parse_search_results(&self, root: &Value, tracks: &mut Vec<Track>) {
        if let Some(contents) = root
            .pointer("/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents")
            .and_then(|v| v.as_array())
        {
            for section in contents {
                if let Some(items) = section
                    .pointer("/musicShelfRenderer/contents")
                    .and_then(|v| v.as_array())
                {
                    for item in items {
                        if let Some(track) = self.extract_track_from_renderer(item) {
                            tracks.push(track);
                        }
                    }
                }
            }
        }
    }

    fn extract_track_from_renderer(&self, item: &Value) -> Option<Track> {
        let renderer = item.get("musicResponsiveListItemRenderer")?;

        let video_id = renderer
            .pointer("/playlistItemData/videoId")
            .and_then(|v| v.as_str())
            .or_else(|| {
                renderer
                    .pointer("/doubleTapCommand/watchEndpoint/videoId")
                    .and_then(|v| v.as_str())
            })
            .or_else(|| {
                renderer
                    .pointer("/navigationEndpoint/watchEndpoint/videoId")
                    .and_then(|v| v.as_str())
            })?
            .to_string();

        let title = renderer
            .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown Title")
            .to_string();

        let artist = renderer
            .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown Artist")
            .to_string();

        let album = renderer
            .pointer("/flexColumns/1/musicResponsiveListItemFlexColumnRenderer/text/runs/2/text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Extract thumbnail
        let thumbnail_url = renderer
            .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails/0/url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Extract duration
        let duration_text = renderer
            .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text")
            .and_then(|v| v.as_str())
            .unwrap_or("0:00")
            .to_string();

        let duration_seconds = Self::parse_duration_str(&duration_text);

        Some(Track {
            id: video_id,
            title,
            artist,
            album,
            duration_seconds,
            duration_text,
            thumbnail_url,
        })
    }

    fn parse_duration_str(d: &str) -> u32 {
        let parts: Vec<&str> = d.split(':').collect();
        match parts.len() {
            2 => {
                let min = parts[0].parse::<u32>().unwrap_or(0);
                let sec = parts[1].parse::<u32>().unwrap_or(0);
                min * 60 + sec
            }
            3 => {
                let hr = parts[0].parse::<u32>().unwrap_or(0);
                let min = parts[1].parse::<u32>().unwrap_or(0);
                let sec = parts[2].parse::<u32>().unwrap_or(0);
                hr * 3600 + min * 60 + sec
            }
            _ => 0,
        }
    }

    /// Extract direct playable audio stream URL for a given video/track ID
    pub async fn get_audio_stream_url(&self, video_id: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Strategy 1: YouTube Music Android client Innertube API
        let url = format!("{}/player", self.base_url);
        let payload = json!({
            "context": {
                "client": {
                    "clientName": "ANDROID_MUSIC",
                    "clientVersion": "6.42.52",
                    "androidSdkVersion": 30,
                    "hl": "en",
                    "gl": "US"
                }
            },
            "videoId": video_id
        });

        if let Ok(res) = self.client.post(&url).json(&payload).send().await {
            if let Ok(json_body) = res.json::<Value>().await {
                if let Some(formats) = json_body
                    .pointer("/streamingData/adaptiveFormats")
                    .and_then(|v| v.as_array())
                {
                    // Find audio stream format (prefer AAC itag 140 or Opus itag 251)
                    let mut best_format: Option<StreamFormat> = None;
                    for f in formats {
                        let mime = f.get("mimeType").and_then(|v| v.as_str()).unwrap_or("");
                        if mime.starts_with("audio/") {
                            let stream_url = f.get("url").and_then(|v| v.as_str());
                            if let Some(raw_url) = stream_url {
                                let bitrate = f.get("bitrate").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                let itag = f.get("itag").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                let audio_quality = f.get("audioQuality").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let content_length = f.get("contentLength").and_then(|v| v.as_str()).and_then(|s| s.parse().ok());

                                let current = StreamFormat {
                                    itag,
                                    mime_type: mime.to_string(),
                                    bitrate,
                                    audio_quality,
                                    url: raw_url.to_string(),
                                    content_length,
                                };

                                if best_format.as_ref().map_or(true, |b| current.bitrate > b.bitrate) {
                                    best_format = Some(current);
                                }
                            }
                        }
                    }

                    if let Some(best) = best_format {
                        log::info!("Found InnerTube direct audio stream: itag {} ({} kbps)", best.itag, best.bitrate / 1000);
                        return Ok(best.url);
                    }
                }
            }
        }

        // Strategy 2: High-availability Piped API audio stream fallback
        let piped_instances = [
            "https://pipedapi.kavin.rocks",
            "https://api.piped.private.coffee",
            "https://piped-api.lunar.icu",
        ];

        for instance in piped_instances {
            let stream_endpoint = format!("{}/streams/{}", instance, video_id);
            if let Ok(res) = self.client.get(&stream_endpoint).send().await {
                if let Ok(json_val) = res.json::<Value>().await {
                    if let Some(audio_streams) = json_val.get("audioStreams").and_then(|v| v.as_array()) {
                        if let Some(best_audio) = audio_streams.iter().max_by_key(|s| s.get("bitrate").and_then(|b| b.as_u64()).unwrap_or(0)) {
                            if let Some(stream_url) = best_audio.get("url").and_then(|u| u.as_str()) {
                                log::info!("Fetched audio stream from Piped instance: {}", instance);
                                return Ok(stream_url.to_string());
                            }
                        }
                    }
                }
            }
        }

        Err("No valid audio stream URL found for this track".into())
    }

    /// Retrieve YouTube Music user's playlists with Google OAuth Access Token
    pub async fn fetch_user_playlists(&self, access_token: &str) -> Result<Vec<Playlist>, Box<dyn std::error::Error + Send + Sync>> {
        let url = "https://www.googleapis.com/youtube/v3/playlists?part=snippet,contentDetails&mine=true&maxResults=50";
        
        let res = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .await?;

        let json_body: Value = res.json().await?;
        let mut playlists = Vec::new();

        // Always include "Liked Music" virtual playlist
        playlists.push(Playlist {
            id: "LL".to_string(), // LL is YouTube's Liked Videos / Liked Music playlist ID
            title: "Liked Music".to_string(),
            description: "Your favorite songs synced from your Google Account".to_string(),
            track_count: 0,
            thumbnail_url: "https://music.youtube.com/img/on_platform_logo.svg".to_string(),
        });

        if let Some(items) = json_body.get("items").and_then(|v| v.as_array()) {
            for item in items {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let title = item.pointer("/snippet/title").and_then(|v| v.as_str()).unwrap_or("Playlist").to_string();
                let description = item.pointer("/snippet/description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let track_count = item.pointer("/contentDetails/itemCount").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let thumbnail_url = item.pointer("/snippet/thumbnails/medium/url")
                    .or_else(|| item.pointer("/snippet/thumbnails/default/url"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                playlists.push(Playlist {
                    id,
                    title,
                    description,
                    track_count,
                    thumbnail_url,
                });
            }
        }

        Ok(playlists)
    }

    /// Retrieve tracks within a specific playlist
    pub async fn fetch_playlist_tracks(&self, playlist_id: &str, access_token: Option<&str>) -> Result<Vec<Track>, Box<dyn std::error::Error + Send + Sync>> {
        let mut tracks = Vec::new();

        if let Some(token) = access_token {
            let url = format!(
                "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet,contentDetails&playlistId={}&maxResults=50",
                playlist_id
            );

            let res = self.client.get(&url).bearer_auth(token).send().await?;
            let json_body: Value = res.json().await?;

            if let Some(items) = json_body.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let video_id = item.pointer("/snippet/resourceId/videoId").and_then(|v| v.as_str()).unwrap_or("");
                    if video_id.is_empty() {
                        continue;
                    }

                    let title = item.pointer("/snippet/title").and_then(|v| v.as_str()).unwrap_or("Unknown Title").to_string();
                    let artist = item.pointer("/snippet/videoOwnerChannelTitle")
                        .or_else(|| item.pointer("/snippet/channelTitle"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown Artist")
                        .replace(" - Topic", "");

                    let thumbnail_url = item.pointer("/snippet/thumbnails/medium/url")
                        .or_else(|| item.pointer("/snippet/thumbnails/default/url"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    tracks.push(Track {
                        id: video_id.to_string(),
                        title,
                        artist,
                        album: "".to_string(),
                        duration_seconds: 0,
                        duration_text: "".to_string(),
                        thumbnail_url,
                    });
                }
            }
        }

        Ok(tracks)
    }
}
