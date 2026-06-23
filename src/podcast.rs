//! Apple Podcast → transcript resolution.
//!
//! Given an `https://podcasts.apple.com/...` link, this module resolves the
//! episode's audio via the public iTunes Lookup API, then produces a transcript
//! using a two-tier strategy:
//!
//!   1. **RSS transcript (preferred)** — many feeds publish a Podcasting 2.0
//!      `<podcast:transcript>` tag pointing at a VTT/SRT/plain-text transcript.
//!      Using it is free and instant (no STT round-trip).
//!   2. **ffmpeg-chunked STT (fallback)** — download the audio, split it into
//!      sub-25MB chunks with ffmpeg, and transcribe each chunk via the existing
//!      `[stt]` provider (`crate::stt::transcribe`).
//!
//! The caller (discord.rs) injects the returned transcript plus a summary
//! instruction as text for the downstream agent (Kiro), which writes the summary.

use crate::config::{PodcastConfig, SttConfig};
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tracing::{debug, error, info, warn};

/// Minutes of audio per ffmpeg segment in the STT fallback. At a typical
/// 128 kbps podcast bitrate this is ~9 MB/segment — comfortably under the 25 MB
/// Whisper limit, with headroom for higher-bitrate feeds.
const SEGMENT_MINUTES: u32 = 10;

/// Hard ceiling on the audio file we'll download for chunked STT (defense
/// against pathologically large enclosures).
const MAX_AUDIO_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Debug)]
pub struct PodcastResult {
    pub title: String,
    pub transcript: String,
}

/// Identifiers extracted from an Apple Podcasts URL.
#[derive(Debug, PartialEq)]
struct AppleIds {
    collection_id: String,
    episode_id: Option<String>,
}

/// What the iTunes Lookup API tells us about the target episode.
#[derive(Debug, Default)]
struct EpisodeInfo {
    title: String,
    /// Direct audio enclosure URL (for STT fallback).
    episode_url: Option<String>,
    /// Publisher RSS feed (for the preferred transcript path).
    feed_url: Option<String>,
}

/// Matches an Apple Podcasts link anywhere in a message.
pub static APPLE_PODCAST_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"https?://podcasts\.apple\.com/\S+").unwrap()
});

/// Resolve an Apple Podcast URL to a transcript. Returns `None` (with a logged
/// reason) on any failure so the caller can proceed without a transcript.
pub async fn fetch_transcript(
    client: &reqwest::Client,
    stt_cfg: &SttConfig,
    podcast_cfg: &PodcastConfig,
    url: &str,
) -> Option<PodcastResult> {
    let ids = parse_apple_url(url)?;
    debug!(?ids, "parsed apple podcast url");

    let info = itunes_lookup(client, &ids).await?;
    debug!(title = %info.title, has_feed = info.feed_url.is_some(), has_audio = info.episode_url.is_some(), "itunes lookup");

    // Tier 1: RSS-embedded transcript.
    if let Some(feed_url) = &info.feed_url {
        if let Some(item) = fetch_rss_item(client, feed_url, info.episode_url.as_deref()).await {
            let title = if info.title.is_empty() { item.title.clone() } else { info.title.clone() };
            if let Some(turl) = &item.transcript_url {
                if let Some(text) = download_transcript(client, turl, item.transcript_type.as_deref()).await {
                    info!(chars = text.len(), "using RSS-embedded transcript");
                    return Some(PodcastResult { title, transcript: text });
                }
            }
            // RSS gave us no transcript but may have given a better audio URL.
            let audio = info.episode_url.clone().or(item.enclosure_url.clone());
            if let Some(audio) = audio {
                if let Some(text) = transcribe_by_chunks(client, stt_cfg, podcast_cfg, &audio).await {
                    return Some(PodcastResult { title, transcript: text });
                }
            }
            return None;
        }
    }

    // Tier 2: chunked STT on the iTunes-provided audio URL.
    let audio = info.episode_url.clone()?;
    let text = transcribe_by_chunks(client, stt_cfg, podcast_cfg, &audio).await?;
    Some(PodcastResult { title: info.title, transcript: text })
}

/// Extract `id{collectionId}` and optional `?i={episodeId}` from an Apple URL.
fn parse_apple_url(url: &str) -> Option<AppleIds> {
    static ID_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"/id(\d+)").unwrap());
    static EP_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"[?&]i=(\d+)").unwrap());

    let collection_id = ID_RE.captures(url)?.get(1)?.as_str().to_string();
    let episode_id = EP_RE.captures(url).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
    Some(AppleIds { collection_id, episode_id })
}

/// Query the public iTunes Lookup API. Prefers the episode id (returns the
/// episode directly); otherwise looks up the collection and takes its latest
/// episode.
async fn itunes_lookup(client: &reqwest::Client, ids: &AppleIds) -> Option<EpisodeInfo> {
    let lookup_id = ids.episode_id.as_deref().unwrap_or(&ids.collection_id);
    let url = format!(
        "https://itunes.apple.com/lookup?id={lookup_id}&entity=podcastEpisode&limit=200"
    );

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        error!(status = %resp.status(), "itunes lookup failed");
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let results = json.get("results")?.as_array()?;

    // Episode entry: the one carrying an audio enclosure (episodeUrl).
    let episode = results.iter().find(|r| r.get("episodeUrl").is_some());
    // feedUrl can live on the episode or the collection entry.
    let feed_url = results
        .iter()
        .find_map(|r| r.get("feedUrl").and_then(|v| v.as_str()))
        .map(str::to_string);

    let mut info = EpisodeInfo { feed_url, ..Default::default() };
    if let Some(ep) = episode {
        info.episode_url = ep.get("episodeUrl").and_then(|v| v.as_str()).map(str::to_string);
        info.title = ep
            .get("trackName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    } else if let Some(first) = results.first() {
        // Collection-only result (no episode entity returned).
        info.title = first
            .get("collectionName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
    }

    if info.episode_url.is_none() && info.feed_url.is_none() {
        return None;
    }
    Some(info)
}

/// One RSS `<item>` we care about.
#[derive(Debug, Default, Clone)]
struct RssItem {
    title: String,
    enclosure_url: Option<String>,
    transcript_url: Option<String>,
    transcript_type: Option<String>,
}

/// Fetch the feed and return the item matching `target_audio` (by enclosure
/// URL), or the first item if no target is given / no match is found.
async fn fetch_rss_item(
    client: &reqwest::Client,
    feed_url: &str,
    target_audio: Option<&str>,
) -> Option<RssItem> {
    let resp = client.get(feed_url).send().await.ok()?;
    if !resp.status().is_success() {
        warn!(status = %resp.status(), "rss fetch failed");
        return None;
    }
    let xml = resp.text().await.ok()?;
    let items = parse_rss_items(&xml);
    if items.is_empty() {
        return None;
    }

    if let Some(target) = target_audio {
        let target_base = strip_query(target);
        if let Some(found) = items.iter().find(|it| {
            it.enclosure_url
                .as_deref()
                .is_some_and(|u| strip_query(u) == target_base)
        }) {
            return Some(found.clone());
        }
    }
    items.into_iter().next()
}

/// Parse RSS items, capturing title, enclosure, and `<podcast:transcript>`.
/// Prefers a VTT/SRT/plain transcript over JSON when several are listed.
fn parse_rss_items(xml: &str) -> Vec<RssItem> {
    let mut reader = Reader::from_str(xml);

    let mut items = Vec::new();
    let mut cur: Option<RssItem> = None;
    let mut in_title = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"item" => cur = Some(RssItem::default()),
                b"title" if cur.is_some() => in_title = true,
                _ => {}
            },
            Ok(Event::Empty(e)) => match e.name().as_ref() {
                b"enclosure" => {
                    if let Some(item) = cur.as_mut() {
                        if let Some(u) = attr(&e, b"url") {
                            item.enclosure_url = Some(u);
                        }
                    }
                }
                b"podcast:transcript" => {
                    if let Some(item) = cur.as_mut() {
                        let url = attr(&e, b"url");
                        let typ = attr(&e, b"type");
                        // Prefer a text transcript over a JSON one if both exist.
                        let better = item.transcript_url.is_none()
                            || typ.as_deref().is_some_and(|t| !t.contains("json"));
                        if url.is_some() && better {
                            item.transcript_url = url;
                            item.transcript_type = typ;
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Text(t)) => {
                if in_title {
                    if let Some(item) = cur.as_mut() {
                        item.title.push_str(&t.unescape().unwrap_or_default());
                    }
                }
            }
            Ok(Event::CData(t)) => {
                if in_title {
                    if let Some(item) = cur.as_mut() {
                        item.title
                            .push_str(&String::from_utf8_lossy(t.as_ref()));
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"title" => in_title = false,
                b"item" => {
                    if let Some(mut item) = cur.take() {
                        item.title = item.title.trim().to_string();
                        items.push(item);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => {
                warn!(error = %e, "rss parse error");
                break;
            }
            _ => {}
        }
    }
    items
}

/// Download a transcript file and normalize it to plain text. VTT/SRT cue
/// timing and markup are stripped.
async fn download_transcript(
    client: &reqwest::Client,
    url: &str,
    typ: Option<&str>,
) -> Option<String> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let raw = resp.text().await.ok()?;
    let is_timed = typ.is_some_and(|t| t.contains("vtt") || t.contains("srt"))
        || raw.contains("-->");
    let text = if is_timed { strip_vtt_timestamps(&raw) } else { raw.trim().to_string() };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Strip WebVTT/SRT scaffolding (header, cue indices, timing lines, inline
/// tags) leaving readable prose.
fn strip_vtt_timestamps(raw: &str) -> String {
    static TAG_RE: LazyLock<regex::Regex> =
        LazyLock::new(|| regex::Regex::new(r"<[^>]*>").unwrap());

    let mut out: Vec<String> = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty()
            || line == "WEBVTT"
            || line.starts_with("NOTE")
            || line.starts_with("STYLE")
            || line.contains("-->")
            || line.chars().all(|c| c.is_ascii_digit())
        {
            continue;
        }
        let cleaned = TAG_RE.replace_all(line, "").trim().to_string();
        if cleaned.is_empty() {
            continue;
        }
        // Skip consecutive duplicate caption lines (common in auto-captions).
        if out.last().is_some_and(|l| l == &cleaned) {
            continue;
        }
        out.push(cleaned);
    }
    out.join("\n")
}

/// Download audio, split into chunks with ffmpeg, and transcribe each via STT.
async fn transcribe_by_chunks(
    client: &reqwest::Client,
    stt_cfg: &SttConfig,
    podcast_cfg: &PodcastConfig,
    audio_url: &str,
) -> Option<String> {
    if stt_cfg.api_key.is_empty() {
        warn!("podcast STT fallback needs an [stt] api_key — skipping");
        return None;
    }

    // Unique temp workspace, cleaned up on the way out.
    let work = std::env::temp_dir().join(format!("openab-podcast-{}", uuid::Uuid::new_v4()));
    if std::fs::create_dir_all(&work).is_err() {
        error!("failed to create podcast temp dir");
        return None;
    }
    let result = transcribe_by_chunks_inner(client, stt_cfg, podcast_cfg, audio_url, &work).await;
    let _ = std::fs::remove_dir_all(&work);
    result
}

async fn transcribe_by_chunks_inner(
    client: &reqwest::Client,
    stt_cfg: &SttConfig,
    podcast_cfg: &PodcastConfig,
    audio_url: &str,
    work: &Path,
) -> Option<String> {
    let ext = audio_ext(audio_url);
    let input = work.join(format!("episode.{ext}"));

    // Download the enclosure to disk.
    let resp = client.get(audio_url).send().await.ok()?;
    if !resp.status().is_success() {
        error!(status = %resp.status(), "audio download failed");
        return None;
    }
    if let Some(len) = resp.content_length() {
        if len > MAX_AUDIO_BYTES {
            error!(len, "audio exceeds size ceiling, skipping");
            return None;
        }
    }
    let bytes = resp.bytes().await.ok()?;
    if std::fs::write(&input, &bytes).is_err() {
        error!("failed to write audio temp file");
        return None;
    }
    debug!(bytes = bytes.len(), "downloaded podcast audio");

    // Split into SEGMENT_MINUTES chunks via ffmpeg stream-copy (fast, lossless).
    let pattern = work.join(format!("chunk_%03d.{ext}"));
    let segment_time = (SEGMENT_MINUTES * 60).to_string();
    let status = tokio::process::Command::new(&podcast_cfg.ffmpeg_path)
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(&input)
        .args(["-f", "segment", "-segment_time", &segment_time, "-c", "copy", "-reset_timestamps", "1"])
        .arg(&pattern)
        .status()
        .await;

    let chunks: Vec<PathBuf> = match status {
        Ok(s) if s.success() => collect_chunks(work, ext),
        Ok(s) => {
            error!(code = ?s.code(), "ffmpeg segment failed");
            return None;
        }
        Err(e) => {
            error!(error = %e, ffmpeg = %podcast_cfg.ffmpeg_path, "ffmpeg not runnable — install ffmpeg or set podcast.ffmpeg_path");
            return None;
        }
    };

    if chunks.is_empty() {
        error!("ffmpeg produced no chunks");
        return None;
    }

    // Cap the number of chunks we transcribe to respect max_minutes.
    let max_chunks = podcast_cfg.max_minutes.div_ceil(SEGMENT_MINUTES).max(1) as usize;
    let total = chunks.len();
    let mime = audio_mime(ext);
    let mut transcript = String::new();
    for (i, chunk) in chunks.into_iter().take(max_chunks).enumerate() {
        let Ok(data) = std::fs::read(&chunk) else { continue };
        let filename = format!("chunk_{i:03}.{ext}");
        match crate::stt::transcribe(client, stt_cfg, data, filename, mime).await {
            Some(part) => {
                if !transcript.is_empty() {
                    transcript.push(' ');
                }
                transcript.push_str(&part);
            }
            None => warn!(chunk = i, "chunk transcription returned nothing"),
        }
    }

    if total > max_chunks {
        info!(transcribed = max_chunks, total, "podcast truncated to max_minutes");
    }
    if transcript.trim().is_empty() {
        None
    } else {
        Some(transcript)
    }
}

/// List `chunk_*.ext` files in numeric order.
fn collect_chunks(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut chunks: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("chunk_") && n.ends_with(ext))
        })
        .collect();
    chunks.sort();
    chunks
}

fn strip_query(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// Lowercased audio file extension from a URL (defaults to `mp3`).
fn audio_ext(url: &str) -> &str {
    let path = strip_query(url);
    match path.rsplit('.').next().map(str::to_ascii_lowercase) {
        Some(ext) => match ext.as_str() {
            "mp3" => "mp3",
            "m4a" => "m4a",
            "mp4" => "mp4",
            "aac" => "aac",
            "ogg" | "oga" => "ogg",
            "wav" => "wav",
            "flac" => "flac",
            _ => "mp3",
        },
        None => "mp3",
    }
}

fn audio_mime(ext: &str) -> &'static str {
    match ext {
        "m4a" | "mp4" | "aac" => "audio/mp4",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        _ => "audio/mpeg",
    }
}

/// Read a string attribute off an XML start/empty tag.
fn attr(e: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_episode_url() {
        let url = "https://podcasts.apple.com/tw/podcast/some-show/id1500000000?i=1000600000000";
        let ids = parse_apple_url(url).unwrap();
        assert_eq!(ids.collection_id, "1500000000");
        assert_eq!(ids.episode_id.as_deref(), Some("1000600000000"));
    }

    #[test]
    fn parses_show_only_url() {
        let url = "https://podcasts.apple.com/us/podcast/the-daily/id1200361736";
        let ids = parse_apple_url(url).unwrap();
        assert_eq!(ids.collection_id, "1200361736");
        assert_eq!(ids.episode_id, None);
    }

    #[test]
    fn parses_url_with_extra_query_params() {
        let url = "https://podcasts.apple.com/gb/podcast/x/id42?i=99&l=en";
        let ids = parse_apple_url(url).unwrap();
        assert_eq!(ids.collection_id, "42");
        assert_eq!(ids.episode_id.as_deref(), Some("99"));
    }

    #[test]
    fn rejects_non_apple_url() {
        assert!(parse_apple_url("https://example.com/podcast/id1").is_none());
    }

    #[test]
    fn strips_vtt_scaffolding() {
        let vtt = "WEBVTT\n\n1\n00:00:00.000 --> 00:00:02.000\nHello world\n\n2\n00:00:02.000 --> 00:00:04.000\n<c>How</c> are you\n";
        let out = strip_vtt_timestamps(vtt);
        assert_eq!(out, "Hello world\nHow are you");
    }

    #[test]
    fn strips_srt_scaffolding() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nFirst line\n\n2\n00:00:03,000 --> 00:00:05,000\nSecond line\n";
        let out = strip_vtt_timestamps(srt);
        assert_eq!(out, "First line\nSecond line");
    }

    #[test]
    fn collapses_duplicate_caption_lines() {
        let vtt = "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nrepeat\n\n00:00:02.000 --> 00:00:04.000\nrepeat\n";
        assert_eq!(strip_vtt_timestamps(vtt), "repeat");
    }

    #[test]
    fn parses_rss_transcript_and_enclosure() {
        let xml = r#"<rss><channel>
            <item>
              <title>Episode One</title>
              <enclosure url="https://cdn.example.com/ep1.mp3" type="audio/mpeg"/>
              <podcast:transcript url="https://cdn.example.com/ep1.vtt" type="text/vtt"/>
            </item>
            <item>
              <title>Episode Two</title>
              <enclosure url="https://cdn.example.com/ep2.mp3"/>
            </item>
        </channel></rss>"#;
        let items = parse_rss_items(xml);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Episode One");
        assert_eq!(items[0].transcript_url.as_deref(), Some("https://cdn.example.com/ep1.vtt"));
        assert_eq!(items[0].enclosure_url.as_deref(), Some("https://cdn.example.com/ep1.mp3"));
        assert!(items[1].transcript_url.is_none());
    }

    #[test]
    fn rss_prefers_text_transcript_over_json() {
        let xml = r#"<rss><channel><item>
            <title>X</title>
            <podcast:transcript url="https://e/ep.json" type="application/json"/>
            <podcast:transcript url="https://e/ep.vtt" type="text/vtt"/>
        </item></channel></rss>"#;
        let items = parse_rss_items(xml);
        assert_eq!(items[0].transcript_url.as_deref(), Some("https://e/ep.vtt"));
    }

    #[test]
    fn audio_ext_handles_query_and_unknown() {
        assert_eq!(audio_ext("https://x/ep.mp3?token=abc"), "mp3");
        assert_eq!(audio_ext("https://x/ep.m4a"), "m4a");
        assert_eq!(audio_ext("https://x/stream"), "mp3");
    }
}
