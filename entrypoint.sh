#!/bin/bash
set -e

# Clear all stale kiro auth to prevent "already logged in" conflict
rm -rf "$HOME/.local/share/kiro-cli" "$HOME/.kiro"

# Restore kiro-cli token from env var if provided
if [ -n "$KIRO_AUTH_B64" ]; then
  mkdir -p "$HOME/.local/share/kiro-cli"
  echo "$KIRO_AUTH_B64" | base64 -d | gunzip > "$HOME/.local/share/kiro-cli/data.sqlite3"
  chmod 600 "$HOME/.local/share/kiro-cli/data.sqlite3"
fi

exec openab "$@"
