# RAIOS Workspace — GT-LCARS adaptation

Implemented 2026-10-05 at the owner's request, using the local `gt-lcars-kde` project as the visual source. Following the owner's function-first request, GT-LCARS is now an optional skin under **Ayarlar → Arayüz**. The native default is the compact neutral workspace. Skin and dark/light tone are independent persisted preferences; open files and PTYs are preserved. Earlier screenshots below document the original visual pass.

## Source and palette

Source: `/home/alaz/dev/tools/gt-lcars-kde/design/tokens.json`, with the current project's memory decisions and Kvantum/wallpaper previews. The latest direction uses cool blue on the left and warm orange on the right; violet/lavender are reserved and unused.

| Role | KDE token | Workspace value |
|---|---|---|
| Canvas | `color.base.black` | `#000000` |
| Recessed panel | `color.base.surfaceLow` | `#0E0E0E` |
| Raised surface | `color.base.surface` | `#1A1A1A` |
| Primary action | `color.accent.primary` | `#FF7000` |
| Hover | `color.accent.light` | `#FFAB73` |
| File/navigation chrome | `color.cool.blue` | `#6699FF` |
| Secondary cool ink | `color.cool.iceBlue` | `#99CCFF` |
| Content ink | `color.text.cream` | `#FFEECC` |
| Muted ink | `color.text.muted` | `#999999` |

The top command rail and file panel use blue; the inspector and agent actions use orange. Rounded outer panel turns and segmented rails adapt the theme's LCARS geometry to a dense editor workspace. Filled controls have black labels, and inactive controls keep legible cream/secondary ink on dark surfaces. Primary buttons use the 36px pill height and 18px radius; dialogs use 24px corners and 3px orange borders. Window controls remain native and inherit the desktop's decoration theme.

Antonio is bundled from the KDE project's existing OFL font asset for display labels, pane titles, and primary actions. Body text and file paths use Noto Sans/system fallbacks; source code and terminals retain monospace. Uppercase treatment applies to display chrome, preserving the case of file paths and editable content. A cream-based light variant adapts text/syntax ink while preserving the same orange/blue fills and black toolbar.

## Font provenance

- Unmodified source: `gt-lcars-kde/src/fonts/Antonio.ttf`.
- Bundled file: `tools/raios-workspace/src/assets/fonts/Antonio.ttf`.
- SHA-256: `9e95a2258ecdf3e45c72c5bbea1c4cd350e8f7bebc87c9dba53b29b1890b8903`.
- Original OFL text is shipped under `tools/raios-workspace/public/licenses/Antonio-OFL.txt` and copied into the frontend bundle. No font download is required at runtime.

## Validation and screens

Frontend build and standalone Tauri debug package passed. The existing eleven client regression checks passed with the real CodeMirror/xterm components and a fixture native transport. All three layouts were checked at 1440×900, 1200×800, and the native minimum 1000×680 without page overflow. Main editor text, muted text, keyword/type/string/number ink meet 4.5:1: the minimum measured ratio was 7.37:1 in dark mode and 6.33:1 in light mode. Black labels on orange/blue fills both measure 7.57:1. Disabled controls are excluded from this claim. The review applied color, typography, layout, and accessibility principles from the local apple-design references.

These screenshots show the implemented client with explicit fixture data; the terminal caption labels it as a UI preview.

- [Dark editor](gt-lcars-dark.png)
- [Light editor](gt-lcars-light.png)
- [Handoff dialog](gt-lcars-handoff.png)

The visual change preserves existing project scopes, file-save conflict checks, PTY ownership, handoff receipts, and approval policy enforcement. Initial project/search controls now start disabled until the client attaches its native listeners, preventing clicks during initialization from being lost.
