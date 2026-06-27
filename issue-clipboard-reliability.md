---
name: Bug Report
about: Report a bug or issue with Windows 11 Clipboard History For Linux
title: '[BUG] Linux clipboard unreliability — orphaned processes, paste failures, Wayland read failures'
labels: ['bug', 'triage']
assignees: ''
---

## 🐛 Bug Description

Three separate clipboard reliability bugs affect Linux users on both X11 and Wayland (observed since v0.5.x):

**Bug 1 — Orphaned clipboard child processes leak.** Every time the app copies text or HTML, it spawns `xclip`/`wl-copy`. Previous children are never killed or waited on. On shutdown, no cleanup happens. Over time (especially with the 500ms clipboard watcher), dozens of orphaned processes accumulate.

**Bug 2 — Clipboard data lost on paste (X11).** `xclip` without `-loops 0` exits immediately after writing to the clipboard. When a paste request arrives later, `xclip` is gone and the clipboard data is lost. Works with a clipboard manager, fails without one.

**Bug 3 — Clipboard read fails on Wayland (wlroots compositors).** `arboard` (via `wayland-data-control`) fails to read the clipboard on wlroots-based compositors (Sway, Hyprland, etc.). There is no fallback, so the clipboard monitor silently stops working.

**Related:** #259 — same root cause as Bug 2 (clipboard owner exits) but for images via `arboard`.

## 📋 Steps to Reproduce

**Bug 1:**
1. Start the app
2. Copy text 5+ times
3. Run `pgrep -c xclip; pgrep -c wl-copy` — shows >1 process
4. Restart the app — processes remain orphaned

**Bug 2 (X11 only, no clipboard manager):**
1. Start the app on X11
2. Copy some text via the app
3. Immediately try to paste
4. The paste is empty or fails

**Bug 3 (Wayland + wlroots):**
1. Start the app on Sway/Hyprland
2. Copy text via any application
3. The app does not detect the new content
4. Check stderr for arboard errors

## ✅ Expected Behavior

1. No orphaned processes accumulate
2. Clipboard data persists until pasted (X11)
3. Wayland clipboard reading falls back to `wl-paste` when `arboard` fails

## ❌ Actual Behavior

1. Orphaned `xclip`/`wl-copy` processes accumulate indefinitely
2. Clipboard is empty when pasting on X11 without clipboard manager
3. Clipboard history stops updating on wlroots-based Wayland sessions

## 📸 Screenshots

N/A

## 🖥️ Environment

- **OS**: Linux (Ubuntu 24.04, Arch, Fedora)
- **Desktop Environment**: GNOME 46, Sway, Hyprland, LXQt
- **Display Server**: [X11 / Wayland]
- **App Version**: 0.5.x – 0.6.6
- **Installation Method**: All methods

## 📝 Additional Context

A branch with all three fixes is available: `consolidated-clipboard-fixes`.
Fixes: track children in `Mutex<Option<Child>>` + `Drop`, add `-loops 0` to xclip, fall back to `wl-paste`.

## 📄 Logs

<details>
<summary>Click to expand logs</summary>

```
# After 5 copies:
$ pgrep -c xclip
5

# Pasting on X11 without clipboard manager:
$ xclip -o -selection clipboard
Error: target STRING not available

# Wayland arboard error (stderr):
[ClipboardManager] arboard read failed: ...
```
</details>
