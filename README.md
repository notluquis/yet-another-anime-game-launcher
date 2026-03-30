# YAAGL — Genshin Impact (Overseas) for macOS

Personal fork of [3Shain/yet-another-anime-game-launcher](https://github.com/3Shain/yet-another-anime-game-launcher), stripped down to **Genshin Impact overseas (hk4eos) only**. Built to optimize my own gameplay experience on Apple Silicon — not a general-purpose launcher.

## What changed from upstream

### Scope
- **Single game**: removed all other clients (CN, HSR, ZZZ, HI3, Seasun, Bilibili). Only GI overseas remains.
- **Smaller codebase**: ~45 deleted files, ~50 dead constants removed, dead UI code stripped.
- **Focused config**: only GI-relevant settings (HDR, FPS unlock, DXMT, ReShade, Steam patch).

### Backend
- **Sophon sidecar rewritten from Python to Rust**: startup dropped from ~20s to ~50ms. No more Python/PyInstaller dependency.
- **DXMT 0.74 builtin mode**: DLLs installed to Wine lib paths instead of system32 overrides.
- **macOS Game Mode**: automatically enables `gamepolicyctl game-mode on` during gameplay.

### Frontend
- **Kobalte** replaces Hope UI — lighter, better Tailwind integration.
- **Vite 8** + Tailwind CSS 4 + SolidJS 1.9 + TypeScript 5.9.
- **Build time**: ~1s production build, 237 modules, ~374 KB JS bundle.

### Fixes applied during audit
- 8 missing `await` on async `setKey`/`writeFile` calls (could silently lose state).
- Dead code branches that could never execute (CN-only paths, removed game checks).
- Orphaned files (.reg, icons, client modules) cleaned up.
- Typos fixed (`contentsss`, `currrent_wine_tag`).

## Supported version

**Genshin Impact OS: 5.3.0+** — Apple Silicon, macOS Sonoma 14.4+ (Sequoia recommended).

## Is it safe?

Use it at your own risk.

## Install

1. Download the latest release.
2. Move the `.app` to `/Applications`.
3. Store game files somewhere in your home folder (e.g. `~/Games/GI`), **not** inside `/Applications`.

## Uninstall

1. Drag the app to Trash.
2. Delete `~/Library/Application Support/Yaagl OS`.

## Related projects

- Original: [3Shain/yet-another-anime-game-launcher](https://github.com/3Shain/yet-another-anime-game-launcher)
- [DXMT](https://github.com/3Shain/DXMT) — Direct3D to Metal translation
- Custom [neutralinojs](https://github.com/3Shain/neutralinojs) binary
- Linux alternative: [Anime Games Launcher](https://github.com/an-anime-team/anime-games-launcher)

## Special thanks

- 3Shain — original YAAGL and Wine/DXMT work
- An Anime Team
- Krock & mkrsym1 — patch work that makes this possible on macOS
