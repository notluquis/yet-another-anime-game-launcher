#!/bin/bash
# test-app.sh — Build and launch a test .app
#
# Uses YAAGL_TEST=1: crea "Yaagl OS Test.app" con su propio estado en
# ~/Library/Application Support/Yaagl OS Test — no toca tu install real.
#
# El launcher de la app ya hace rsync del nuevo código al abrir,
# así que entre runs solo necesitas resetear lo que quedó colgado.
#
# USAGE:
#   ./test-app.sh hk4eos            # build + lanza (resetea solo "patched" si quedó colgado)
#   ./test-app.sh hk4eos --wine     # + resetea wineprefix (fuerza re-setup de Wine)
#   ./test-app.sh hk4eos --full     # + borra Wine, DXMT (re-descarga todo)
#   ./test-app.sh hk4eos --no-reset # solo build y abre, sin tocar estado
#
# CHANNELS: hk4ecn  hk4eos  hkrpgcn  hkrpgos  napcn  napos  bh3glb  cbjq  cbjqcn

set -e

CHANNEL="${1:-hk4eos}"
MODE="${2:-}"
APP_SUPPORT="$HOME/Library/Application Support"

case "$CHANNEL" in
  hk4ecn)   APP_NAME="Yaagl Test" ;;
  hk4eos)   APP_NAME="Yaagl OS Test" ;;
  hkrpgcn)  APP_NAME="Yaagl HSR Test" ;;
  hkrpgos)  APP_NAME="Yaagl HSR OS Test" ;;
  napcn)    APP_NAME="Yaagl ZZZ Test" ;;
  napos)    APP_NAME="Yaagl ZZZ OS Test" ;;
  bh3glb)   APP_NAME="Yaagl Honkai Global Test" ;;
  cbjq)     APP_NAME="Yaagl SCZ OS Test" ;;
  cbjqcn)   APP_NAME="Yaagl SCZ Test" ;;
  *)
    echo "Unknown channel: $CHANNEL"
    echo "Valid: hk4ecn hk4eos hkrpgcn hkrpgos napcn napos bh3glb cbjq cbjqcn"
    exit 1
    ;;
esac

APP_BUNDLE="${APP_NAME}.app"
STATE_DIR="$APP_SUPPORT/$APP_NAME"

echo "======================================"
echo "  Channel:  $CHANNEL"
echo "  App:      $APP_BUNDLE"
echo "  State:    $STATE_DIR"
echo "  Mode:     ${MODE:-default}"
echo "======================================"

# --- 1. Build ---
echo ""
echo ">>> Building..."
YAAGL_CHANNEL_CLIENT="$CHANNEL" YAAGL_TEST=1 node build-app.js
echo "Build done."

# --- 2. Reset (solo lo que puede quedar colgado) ---
if [ "$MODE" != "--no-reset" ]; then

  # "patched" solo existe si el juego se lanzó y el proceso murió antes de revertir.
  # Bajo operación normal se limpia solo — aquí lo forzamos por si acaso.
  PATCHED_KEY="$STATE_DIR/.storage/patched.neustorage"
  if [ -f "$PATCHED_KEY" ]; then
    echo ""
    echo ">>> Limpiando key 'patched' (quedó colgada de sesión anterior)"
    rm "$PATCHED_KEY"
  fi

  # Archivos temporales que pueden quedar si la sesión crasheó
  for f in config.bat fix_webview.reg fix_webview.log fix_resolution.reg fix_resolution.log \
            hk4e_revert_hdr.reg hk4e_revert_resolution.reg sidecar.tar.gz resources.neu.update; do
    [ -f "$STATE_DIR/$f" ] && rm "$STATE_DIR/$f" && echo ">>> Removed leftover: $f"
  done

  # --wine: fuerza re-setup completo de Wine (útil si cambiaste algo en wine-install-program.ts)
  if [ "$MODE" = "--wine" ] || [ "$MODE" = "--full" ]; then
    echo ""
    echo ">>> Reseteando Wine..."
    rm -f "$STATE_DIR/.storage/wine_state.neustorage"
    rm -f "$STATE_DIR/.storage/wine_tag.neustorage"
    rm -f "$STATE_DIR/.storage/wine_netbiosname.neustorage"
    [ -d "$STATE_DIR/wineprefix" ] && rm -rf "$STATE_DIR/wineprefix" && echo "  Removed wineprefix/"
    [ -d "$STATE_DIR/wine" ]       && rm -rf "$STATE_DIR/wine"       && echo "  Removed wine/"
  fi

  # --full: además borra DXMT y otros binarios descargados
  if [ "$MODE" = "--full" ]; then
    echo ""
    echo ">>> Reseteando binarios descargados..."
    for d in dxmt dxvk reshade fpsunlock; do
      [ -d "$STATE_DIR/$d" ] && rm -rf "$STATE_DIR/$d" && echo "  Removed $d/"
    done
    rm -f "$STATE_DIR/.storage/installed_dxmt_version.neustorage"
    echo "  (se re-descargarán al abrir)"
  fi

fi

# --- 3. Launch ---
echo ""
echo ">>> Abriendo $APP_BUNDLE..."
open "$APP_BUNDLE"
