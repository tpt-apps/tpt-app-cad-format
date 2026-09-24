#!/usr/bin/env bash
# TPT CAD Format - build pipeline (macOS/Linux mirror of build.ps1).
#   ./build.sh        free edition -> dist/web (set TPT_WEB_REPO for the hub repo)
#   ./build.sh --pro  pro edition  -> dist/ + native webview exe
set -euo pipefail
cd "$(dirname "$0")"

CRATE=tpt-app-cad-format
TARGET=wasm32-unknown-unknown
OUT=cad-format
DEFAULT_WEB_REPO="$HOME/Programming/2 WIP/TPT Electrician/tpt-electrician-nz-2"

if [ "${1:-}" = "--pro" ]; then
  echo "== TPT CAD Format build: pro =="
  trunk build --release --features pro
  cargo build --release -p tpt-cad-desktop
  mkdir -p release
  cp target/release/tpt-cad-format-pro release/tpt-cad-format-pro
  echo "-- done (public/apps/cad-format/ not touched)"
  exit 0
fi

WEB_REPO="${TPT_WEB_REPO:-}"
if [ -z "$WEB_REPO" ] && [ -d "$DEFAULT_WEB_REPO" ]; then WEB_REPO="$DEFAULT_WEB_REPO"; fi
DEST="$PWD/dist/web"
if [ -n "$WEB_REPO" ]; then DEST="$WEB_REPO/public/apps/cad-format"; fi

echo "== TPT CAD Format build: free (dest: $DEST) =="
cargo build --release --target "$TARGET" -p "$CRATE"
mkdir -p "$DEST"
wasm-bindgen --target web --out-name "$OUT" --out-dir "$DEST" \
  "target/$TARGET/release/tpt_app_cad_format.wasm"

RAW=$(wc -c < "$DEST/${OUT}_bg.wasm" | tr -d ' ')
GZ=$(gzip -c "$DEST/${OUT}_bg.wasm" | wc -c | tr -d ' ')
echo "-- ${OUT}_bg.wasm: $RAW raw, $GZ gzipped ($((GZ / 1024)) KB gz)"
[ "$GZ" -le 2097152 ] || echo "WARNING: payload exceeds the ~2 MB gzip budget" >&2
echo "-- done"
