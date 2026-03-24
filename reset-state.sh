#!/bin/bash
# reset-state.sh — Clean launcher state for fresh testing
#
# TARGETS:
#   Dev mode working dirs:  yaaglwd, yaaglwdos, hkrpgos, napcn, napos, etc.
#   Production app dirs:    ~/Library/Application Support/Yaagl OS
#                           ~/Library/Application Support/Yaagl-OS
#                           ~/Library/Application Support/Yaagl ZZZ OS
#                           etc.
#
# USAGE:
#   ./reset-state.sh                      # soft reset on ~/Library/.../"Yaagl OS"
#   ./reset-state.sh "Yaagl ZZZ OS"       # soft reset on a specific app support dir
#   ./reset-state.sh yaaglwdos            # soft reset on a dev working dir
#   ./reset-state.sh "Yaagl OS" --full    # full reset (wipes wine + dxmt + prefix)
#   ./reset-state.sh "Yaagl OS" --wine    # only reset wineprefix
#   ./reset-state.sh --list               # list all known state directories

APP_SUPPORT="$HOME/Library/Application Support"

# Known production app names (from build-app.js)
KNOWN_APPS=(
  "Yaagl"
  "Yaagl OS"
  "Yaagl Uni"
  "Yaagl HSR"
  "Yaagl HSR OS"
  "Yaagl Honkai Global"
  "Yaagl ZZZ OS"
  "Yaagl ZZZ"
  "Yaagl SCZ OS"
  "Yaagl SCZ"
)

# Known dev working dirs
KNOWN_DEV=(
  "yaaglwd"
  "yaaglwdos"
  "hkrpgcn"
  "hkrpgos"
  "napcn"
  "napos"
  "bh3glb"
)

# --- List mode ---
if [ "$1" = "--list" ]; then
  echo "Production app support directories:"
  for name in "${KNOWN_APPS[@]}"; do
    dir="$APP_SUPPORT/$name"
    if [ -d "$dir" ]; then
      size=$(du -sh "$dir" 2>/dev/null | cut -f1)
      echo "  [EXISTS $size]  $dir"
    fi
  done
  # Also check quick-build.sh style (Yaagl-OS)
  for dir in "$APP_SUPPORT"/Yaagl-*; do
    [ -d "$dir" ] && echo "  [EXISTS $(du -sh "$dir" 2>/dev/null | cut -f1)]  $dir"
  done
  echo ""
  echo "Dev working directories:"
  for name in "${KNOWN_DEV[@]}"; do
    if [ -d "$name" ]; then
      size=$(du -sh "$name" 2>/dev/null | cut -f1)
      echo "  [EXISTS $size]  $name"
    fi
  done
  exit 0
fi

# --- Resolve target directory ---
ARG1="${1:-Yaagl OS}"
MODE="${2:-}"

# Is it an absolute path?
if [[ "$ARG1" == /* ]]; then
  WORKDIR="$ARG1"
# Is it a relative path that exists locally (dev mode)?
elif [ -d "$ARG1" ]; then
  WORKDIR="$(pwd)/$ARG1"
# Otherwise assume it's a production app name
else
  WORKDIR="$APP_SUPPORT/$ARG1"
fi

if [ ! -d "$WORKDIR" ]; then
  echo "Error: directory not found: '$WORKDIR'"
  echo "Run './reset-state.sh --list' to see known directories."
  exit 1
fi

echo "==> Target: $WORKDIR"
echo "==> Mode:   ${MODE:-soft}"

# --- Storage keys ---
echo ""
echo "--- Clearing Neutralino storage ---"
STORAGE_DIR="$WORKDIR/.storage"
if [ -d "$STORAGE_DIR" ]; then
  FILES=("$STORAGE_DIR"/*.neustorage)
  if [ -f "${FILES[0]}" ]; then
    echo "  Removing ${#FILES[@]} key(s):"
    for f in "${FILES[@]}"; do
      echo "    - $(basename "$f")"
      rm "$f"
    done
  else
    echo "  (already empty)"
  fi
else
  echo "  (no .storage directory)"
fi

# --- Temp/patch files ---
echo ""
echo "--- Clearing temp/patch files ---"
TEMP_FILES=(
  "config.bat"
  "fix_webview.reg"
  "fix_webview.log"
  "fix_resolution.reg"
  "fix_resolution.log"
  "hk4e_revert_hdr.reg"
  "hk4e_revert_resolution.reg"
  "sidecar.tar.gz"
  "resources.neu.update"
)
REMOVED=0
for f in "${TEMP_FILES[@]}"; do
  if [ -f "$WORKDIR/$f" ]; then
    echo "  Removing $f"
    rm "$WORKDIR/$f"
    REMOVED=$((REMOVED + 1))
  fi
done
[ "$REMOVED" -eq 0 ] && echo "  (none found)"

# --- Wine prefix ---
if [ "$MODE" = "--wine" ] || [ "$MODE" = "--full" ]; then
  echo ""
  echo "--- Resetting wineprefix ---"
  if [ -d "$WORKDIR/wineprefix" ]; then
    echo "  Removing wineprefix/ ($(du -sh "$WORKDIR/wineprefix" | cut -f1)) ..."
    rm -rf "$WORKDIR/wineprefix"
    echo "  Done."
  else
    echo "  (no wineprefix)"
  fi
fi

# --- Full reset: downloaded binaries ---
if [ "$MODE" = "--full" ]; then
  echo ""
  echo "--- Full reset: removing downloaded binaries ---"
  FULL_REMOVE=("wine" "dxmt" "dxvk" "reshade" "fpsunlock" "logs")
  REMOVED=0
  for d in "${FULL_REMOVE[@]}"; do
    if [ -d "$WORKDIR/$d" ]; then
      echo "  Removing $d/ ($(du -sh "$WORKDIR/$d" | cut -f1)) ..."
      rm -rf "$WORKDIR/$d"
      REMOVED=$((REMOVED + 1))
    fi
  done
  [ "$REMOVED" -eq 0 ] && echo "  (none found)"
  echo ""
  echo "  NOTE: Wine and DXMT will be re-downloaded on next launch."
fi

echo ""
echo "==> Done. Launch the app to start fresh."
