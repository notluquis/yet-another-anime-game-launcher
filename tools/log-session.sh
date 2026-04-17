#!/usr/bin/env bash
# log-session.sh — capture a synchronized multi-source log session for a
# Genshin/Wine/DXMT run. Designed for Apple Silicon macOS 13+.
#
# Layers captured:
#   1. Unified logging (log stream) — Metal, CoreAnimation, WindowServer, Wine
#   2. powermetrics (requires sudo) — GPU/CPU power, thermal, per-process
#   3. Optional: xctrace Game Performance template (requires Xcode)
#   4. Optional: fs_usage per-pid (requires sudo)
#
# Usage:
#   tools/log-session.sh [options]
#
# Options:
#   --name NAME        Session name (default: timestamp)
#   --full             Enable powermetrics + fs_usage (needs sudo)
#   --xctrace          Also record with xctrace Game Performance template
#   --pid PID          Attach to a specific PID for fs_usage/sample
#   --duration SEC     Run for SEC seconds then stop (default: until Enter)
#   --outdir DIR       Base output directory (default: ~/yaagl-sessions)
#
# Examples:
#   tools/log-session.sh                                # passive, press Enter to stop
#   sudo tools/log-session.sh --full                    # full power/thermal trace
#   tools/log-session.sh --xctrace --duration 120       # 2 min Instruments trace
#
# After the session, see summary.md in the output directory for analysis
# hints and the raw files for deep dives.

set -euo pipefail

SESSION_NAME=""
FULL_MODE=false
XCTRACE_MODE=false
ATTACH_PID=""
DURATION=""
OUTBASE="$HOME/yaagl-sessions"

while [ $# -gt 0 ]; do
  case "$1" in
    --name) SESSION_NAME="$2"; shift 2 ;;
    --full) FULL_MODE=true; shift ;;
    --xctrace) XCTRACE_MODE=true; shift ;;
    --pid) ATTACH_PID="$2"; shift 2 ;;
    --duration) DURATION="$2"; shift 2 ;;
    --outdir) OUTBASE="$2"; shift 2 ;;
    -h|--help) sed -n '2,30p' "$0"; exit 0 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

: "${SESSION_NAME:=$(date +%Y%m%d-%H%M%S)}"
SDIR="$OUTBASE/$SESSION_NAME"
mkdir -p "$SDIR"

cyan() { printf '\033[36m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }
red() { printf '\033[31m%s\033[0m\n' "$*" >&2; }

cyan "Session: $SDIR"

# -- meta --------------------------------------------------------------
{
  echo "# Session $SESSION_NAME"
  echo
  echo "## uname"
  uname -a
  echo
  echo "## sw_vers"
  sw_vers
  echo
  echo "## CPU/GPU"
  sysctl -n machdep.cpu.brand_string
  system_profiler SPDisplaysDataType 2>/dev/null | head -40
  echo
  echo "## Thermal pressure at start"
  pmset -g therm 2>/dev/null || true
} > "$SDIR/system.md"

# -- unified logging (always on, no sudo) -----------------------------
# Predicate is deliberately wide; filter at analysis time.
LOG_PREDICATE='(subsystem BEGINSWITH "com.apple.Metal") OR (subsystem BEGINSWITH "com.apple.CoreAnimation") OR (subsystem BEGINSWITH "com.apple.WindowServer") OR (subsystem BEGINSWITH "com.apple.gpuaccel") OR (subsystem BEGINSWITH "com.apple.skywalk") OR (process BEGINSWITH "wine") OR (process CONTAINS[c] "Genshin") OR (process CONTAINS[c] "YuanShen") OR (process == "Yaagl") OR (process == "WindowServer")'

cyan "Starting log stream..."
log stream --style ndjson --level debug --predicate "$LOG_PREDICATE" \
  > "$SDIR/system.ndjson" 2> "$SDIR/log.err" &
LOG_PID=$!
green "  log stream pid=$LOG_PID"

# -- powermetrics (needs sudo) ----------------------------------------
PM_PID=""
if $FULL_MODE; then
  if [ "$(id -u)" -ne 0 ]; then
    red "--full requires sudo. Re-run with: sudo $0 --full"
    kill "$LOG_PID" 2>/dev/null || true
    exit 1
  fi
  cyan "Starting powermetrics..."
  powermetrics \
    --samplers cpu_power,gpu_power,thermal,tasks \
    --show-process-gpu \
    --show-process-energy \
    -i 1000 \
    -f plist \
    -o "$SDIR/powermetrics.plist" \
    2> "$SDIR/powermetrics.err" &
  PM_PID=$!
  green "  powermetrics pid=$PM_PID"
fi

# -- fs_usage per-pid (needs sudo, only if --pid given) ---------------
FS_PID=""
if $FULL_MODE && [ -n "$ATTACH_PID" ]; then
  cyan "Starting fs_usage on pid=$ATTACH_PID..."
  fs_usage -w -f filesys "$ATTACH_PID" > "$SDIR/fs_usage.log" 2> "$SDIR/fs_usage.err" &
  FS_PID=$!
  green "  fs_usage pid=$FS_PID"
fi

# -- xctrace (Instruments) --------------------------------------------
XC_PID=""
if $XCTRACE_MODE; then
  cyan "Starting xctrace..."
  # We record system-wide; filter by process later in Instruments UI.
  xctrace record \
    --template 'Game Performance' \
    --all-processes \
    --output "$SDIR/trace.trace" \
    > "$SDIR/xctrace.log" 2>&1 &
  XC_PID=$!
  green "  xctrace pid=$XC_PID"
fi

# -- sampling snapshot helper -----------------------------------------
sample_snapshot() {
  local pid="$1"
  local tag="$2"
  [ -z "$pid" ] && return 0
  sample "$pid" 3 -file "$SDIR/sample-$tag-$(date +%H%M%S).txt" \
    >/dev/null 2>&1 || true
}

# -- periodic snapshots if attached to a pid --------------------------
if [ -n "$ATTACH_PID" ]; then
  (
    while kill -0 "$ATTACH_PID" 2>/dev/null; do
      sleep 30
      sample_snapshot "$ATTACH_PID" "periodic"
    done
  ) &
  SAMPLE_LOOP_PID=$!
fi

# -- cleanup -----------------------------------------------------------
cleanup() {
  yellow ""
  yellow "Stopping loggers..."
  kill "$LOG_PID" 2>/dev/null || true
  [ -n "$PM_PID" ] && kill "$PM_PID" 2>/dev/null || true
  [ -n "$FS_PID" ] && kill "$FS_PID" 2>/dev/null || true
  [ -n "$XC_PID" ] && { kill -INT "$XC_PID" 2>/dev/null; wait "$XC_PID" 2>/dev/null; } || true
  [ -n "${SAMPLE_LOOP_PID:-}" ] && kill "$SAMPLE_LOOP_PID" 2>/dev/null || true
  wait 2>/dev/null || true

  # -- summary ---------------------------------------------------------
  {
    echo "# Summary $SESSION_NAME"
    echo
    echo "## Sizes"
    du -sh "$SDIR"/* 2>/dev/null | sort -k1h
    echo
    echo "## Event counts by subsystem (top 20)"
    if [ -s "$SDIR/system.ndjson" ]; then
      grep -oE '"subsystem":"[^"]*"' "$SDIR/system.ndjson" 2>/dev/null \
        | sort | uniq -c | sort -rn | head -20
    fi
    echo
    echo "## Processes seen"
    if [ -s "$SDIR/system.ndjson" ]; then
      grep -oE '"processImagePath":"[^"]*"' "$SDIR/system.ndjson" 2>/dev/null \
        | sort -u | head -30
    fi
    echo
    echo "## Analysis cheat-sheet"
    cat <<EOF

### What do I look at first?
1.  open $SDIR/system.md                  # hardware context
2.  less $SDIR/system.ndjson              # unified log (ndjson, one event per line)
3.  open $SDIR/trace.trace                # Instruments UI (if --xctrace)

### Find Metal pipeline compiles (PSO build stalls)
  grep -E 'MTLCompiler|newRenderPipelineState|newComputePipelineState' \\
       $SDIR/system.ndjson | head -50

### Find Direct -> Composited transitions (the hypothesis we chase)
  grep -iE 'captured|composit|direct|layer_surface' \\
       $SDIR/system.ndjson | grep -i WindowServer

### Find Wine syscall errors / SEH
  grep -iE 'err:|seh|exception' $SDIR/system.ndjson | head

### GPU/CPU/thermal peaks (needs --full)
  # powermetrics writes a stream of plists separated by NUL bytes.
  # Split then pretty-print with plutil:
  tr '\0' '\n' < $SDIR/powermetrics.plist | \\
    awk '/<\/plist>/{print; exit} {print}' | plutil -p -
  # Or inspect all GPU energy fields across samples:
  tr '\0' '\n' < $SDIR/powermetrics.plist | \\
    grep -E 'gpu_energy|gpu_freq|gpu_idle_ratio|thermal_pressure' | head -40

### File access hotspots (needs --full --pid)
  awk '{print \$5}' $SDIR/fs_usage.log | sort | uniq -c | sort -rn | head
EOF
  } > "$SDIR/summary.md"

  green ""
  green "Done. Output: $SDIR"
  green "See $SDIR/summary.md for analysis cheat-sheet."
}
trap cleanup EXIT INT TERM

# -- wait --------------------------------------------------------------
green ""
green "Recording. Launch Yaagl/Genshin now."
if [ -n "$DURATION" ]; then
  yellow "Will stop automatically after ${DURATION}s."
  sleep "$DURATION"
else
  yellow "Press Enter to stop."
  read -r || true
fi
