#!/bin/sh
set -e

CONFIG=/etc/openab/config.toml
mkdir -p /etc/openab

cat > "$CONFIG" << EOF
[discord]
bot_token = "${DISCORD_BOT_TOKEN}"
allowed_channels = ["${DISCORD_CHANNEL_ID}"]

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
