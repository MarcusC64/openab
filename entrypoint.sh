#!/bin/bash
set -e

# Restore kiro-cli token from env var if provided
if [ -n "$KIRO_AUTH_B64" ]; then
  mkdir -p "$HOME/.local/share/kiro-cli"
  echo "$KIRO_AUTH_B64" | base64 -d | gunzip > "$HOME/.local/share/kiro-cli/data.sqlite3"
  chmod 600 "$HOME/.local/share/kiro-cli/data.sqlite3"
fi

# Clear active session so kiro-cli ACP can re-authenticate with stored tokens
kiro-cli logout 2>/dev/null || true

exec openab "$@"
