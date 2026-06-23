# Apple Podcast Summarization

openab can turn an Apple Podcasts link pasted into chat into a transcript and
hand it to your ACP agent (Kiro CLI, Claude Code, etc.) with a summary
instruction. The agent writes the actual summary — openab just resolves the
episode and produces the transcript.

> Currently wired for the **Discord** adapter. The resolver (`src/podcast.rs`)
> is platform-agnostic, so adding it to Slack/gateway is a small follow-up.

## Quick Start

```toml
[stt]
enabled = true
api_key = "${GROQ_API_KEY}"

[podcast]
enabled = true
```

Then in an allowed channel, @mention the bot with an Apple Podcasts link:

```
@openab https://podcasts.apple.com/tw/podcast/some-show/id1500000000?i=1000600000000
```

The agent replies with a Traditional-Chinese TL;DR plus bullet-point highlights
(customizable via `summary_prompt`).

## How It Works

```
message contains podcasts.apple.com/...
        │
        ▼
  parse id{collection} + ?i={episode}
        │
        ▼
  iTunes Lookup API → episode audio URL + RSS feed URL
        │
        ├─ Tier 1: RSS <podcast:transcript> (VTT/SRT/plain)  ──┐  preferred, free
        │                                                       │
        └─ Tier 2: download audio → ffmpeg split → STT chunks ──┤  fallback
                                                                ▼
        transcript injected as text + summary instruction
                                                                ▼
                                ACP agent (Kiro) produces the summary
```

1. **RSS transcript (preferred).** Many Podcasting 2.0 feeds publish a
   `<podcast:transcript>` tag. openab downloads it and strips VTT/SRT timing —
   no audio processing, no STT cost.
2. **Chunked STT (fallback).** If no published transcript exists, openab
   downloads the audio, splits it into ~10-minute chunks with `ffmpeg`, and
   transcribes each chunk through the OpenAI-compatible `/audio/transcriptions`
   endpoint configured in [`[stt]`](stt.md). Total audio is capped by
   `max_minutes`.

## Configuration Reference

```toml
[podcast]
enabled = true            # default: false
max_minutes = 90          # cap on audio minutes sent to STT (fallback path)
ffmpeg_path = "ffmpeg"    # ffmpeg executable used to split long audio
summary_prompt = "..."    # instruction prepended to the transcript
```

| Field | Required | Default | Description |
|---|---|---|---|
| `enabled` | no | `false` | Enable/disable podcast handling. |
| `max_minutes` | no | `90` | Upper bound on audio transcribed in the STT fallback (guards cost). RSS transcripts are not affected. |
| `ffmpeg_path` | no | `ffmpeg` | Path to the ffmpeg binary. Only needed for the audio fallback. |
| `summary_prompt` | no | zh-TW TL;DR + bullets | Text prepended to the transcript telling the agent how to summarize. |

Transcription credentials come from the shared [`[stt]`](stt.md) block
(`api_key` / `model` / `base_url`) — there is no separate podcast STT config.

## Requirements

- **`[stt]` configured** — required for the audio fallback. If `[stt]` has no
  API key, only feeds that ship their own RSS transcript will work.
- **`ffmpeg` on PATH** — required for the audio fallback. The provided
  Dockerfiles install it. On startup openab logs a warning if `[podcast]` is
  enabled but ffmpeg is not runnable; RSS-transcript feeds still work without it.

## Notes

- Long episodes are truncated to `max_minutes`; a log line records when this
  happens.
- If transcript resolution fails for any reason, the message is still forwarded
  to the agent unchanged (the link just won't be summarized).
- The link can appear anywhere in the message alongside other text.
