#!/bin/bash
set -e

# Restore kiro-cli token from env var if provided
if [ -n "$KIRO_AUTH_B64" ]; then
  echo "[entrypoint] Restoring kiro-cli token..."
  mkdir -p "$HOME/.local/share/kiro-cli"
  printf '%s' "$KIRO_AUTH_B64" | tr -d ' \n\r\t' | tr -- '-_' '+/' | base64 -d --ignore-garbage | gunzip > "$HOME/.local/share/kiro-cli/data.sqlite3"
  chmod 600 "$HOME/.local/share/kiro-cli/data.sqlite3"
  SIZE=$(wc -c < "$HOME/.local/share/kiro-cli/data.sqlite3")
  echo "[entrypoint] Token restored: ${SIZE} bytes at $HOME/.local/share/kiro-cli/data.sqlite3"
else
  echo "[entrypoint] KIRO_AUTH_B64 not set, skipping token restore"
fi

echo "[entrypoint] Testing kiro-cli auth..."
kiro-cli doctor 2>&1 | head -5 || true

exec openab "$@"
