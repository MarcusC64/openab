#!/bin/sh
set -e

CONFIG=/etc/openab/config.toml
mkdir -p /etc/openab

# Start embedded gateway if Telegram is configured
if [ -n "$TELEGRAM_BOT_TOKEN" ]; then
    GATEWAY_LISTEN=0.0.0.0:8080 \
    TELEGRAM_BOT_TOKEN="$TELEGRAM_BOT_TOKEN" \
    TELEGRAM_SECRET_TOKEN="$TELEGRAM_SECRET_TOKEN" \
    GATEWAY_WS_TOKEN="$GATEWAY_WS_TOKEN" \
    openab-gateway &

    GATEWAY_WS_URL="ws://127.0.0.1:8080/ws"
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

if [ -n "$GATEWAY_WS_URL" ]; then
    cat >> "$CONFIG" << EOF

[gateway]
url = "${GATEWAY_WS_URL}"
platform = "${GATEWAY_PLATFORM:-telegram}"
token = "${GATEWAY_WS_TOKEN}"
bot_username = "${TELEGRAM_BOT_USERNAME}"
EOF
fi

if [ -n "$GROQ_API_KEY" ]; then
    cat >> "$CONFIG" << EOF

[stt]
enabled = true
api_key = "${GROQ_API_KEY}"
EOF
fi

exec openab run -c "$CONFIG"
