#!/bin/sh
set -e

CONFIG=/etc/openab/config.toml
mkdir -p /etc/openab

# Unified build: the Telegram webhook adapter is compiled into the openab
# binary and auto-activates when TELEGRAM_BOT_TOKEN is set. The embedded HTTP
# server listens on GATEWAY_LISTEN (default 0.0.0.0:8080) and serves the
# webhook at /webhook/telegram — no separate gateway process required.
if [ -n "$TELEGRAM_BOT_TOKEN" ]; then
    export GATEWAY_LISTEN="${GATEWAY_LISTEN:-0.0.0.0:8080}"
fi

cat > "$CONFIG" << EOF
[discord]
bot_token = "${DISCORD_BOT_TOKEN}"
EOF

# Only set allowed_channels if DISCORD_CHANNEL_ID is non-empty
# (empty = allow all channels; [""] would cause a parse error)
if [ -n "$DISCORD_CHANNEL_ID" ]; then
    cat >> "$CONFIG" << EOF
allowed_channels = ["${DISCORD_CHANNEL_ID}"]
EOF
fi

cat >> "$CONFIG" << EOF

[agent]
command = "kiro-cli"
args = ["acp", "--trust-all-tools"]
working_dir = "/home/agent"

[pool]
max_sessions = ${OPENAB_MAX_SESSIONS:-10}
session_ttl_hours = ${OPENAB_SESSION_TTL_HOURS:-24}

[reactions]
enabled = true
remove_after_reply = false
EOF

if [ -n "$GROQ_API_KEY" ]; then
    cat >> "$CONFIG" << EOF

[stt]
enabled = true
api_key = "${GROQ_API_KEY}"
EOF
fi

# Apple Podcast summarization (Discord). Reuses [stt] above for audio
# transcription, so it's enabled by default when GROQ_API_KEY is present.
# Set OPENAB_PODCAST=false to disable.
if [ -n "$GROQ_API_KEY" ] && [ "${OPENAB_PODCAST:-true}" = "true" ]; then
    cat >> "$CONFIG" << EOF

[podcast]
enabled = true
EOF
fi

exec openab run -c "$CONFIG"
