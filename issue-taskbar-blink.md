---
name: Bug Report
about: Report a bug or issue with Windows 11 Clipboard History For Linux
title: '[BUG] Taskbar icon blinks rapidly on GNOME when app starts minimized to tray'
labels: ['bug', 'triage']
assignees: ''
---

## 🐛 Bug Description

When the app is configured to start minimized to the system tray (via `--background`, typically used in autostart), the taskbar/panel icon blinks rapidly for 1–2 seconds on GNOME. The icon appears and disappears repeatedly before finally staying hidden. This happens on **both** GNOME on X11 and GNOME on Wayland.

**Related:** #139 — duplicate tray icons after suspend on GNOME. Different mechanism (tray vs taskbar, suspend vs startup) but same environment. This bug is not #139.

## 📋 Steps to Reproduce

1. Configure the app to autostart with `--background`
2. Log out and log back in (or restart GNOME Shell)
3. Watch the taskbar during the first 2 seconds after login

Or reproduce manually:
```
killall win11-clipboard-history-bin
WIN11_CLIPBOARD_HISTORY_BIN --background &
# Observe taskbar for 2 seconds
```

## ✅ Expected Behavior

Starting with `--background` should not cause any taskbar icon activity. The icon should appear only when the user first opens the app (via Super+V or tray click).

## ❌ Actual Behavior

The taskbar icon flickers on and off rapidly for ~2 seconds after login/autostart, then stabilizes. After the user opens the app for the first time, everything behaves normally.

## 📸 Screenshots

N/A (visual flickering, not capturable in still screenshot)

## 🖥️ Environment

- **OS**: Ubuntu 24.04, Fedora 39+
- **Desktop Environment**: GNOME 45–46
- **Display Server**: [X11 / Wayland]
- **App Version**: all versions using `--background` flag
- **Installation Method**: All methods

## 📝 Additional Context

**Root cause:** A focus→hide→refocus loop between GNOME Mutter and the app's focus handlers:
1. App window is created by Tauri/GTK
2. App immediately calls `hide()` (background mode)
3. Mutter auto-focuses the new window
4. `Focused(true)` handler fires → `hide()` again
5. `hide()` triggers `Focused(false)` → also calls `hide()`
6. Mutter sees window hidden while focused → tries to restore
7. Background enforcer thread (200ms poll) sees visible → hides again
8. Loop until enforcer gives up or user toggles

A fix is available on branch `fix/taskbar-blink-background-startup`: calls `set_skip_taskbar(true)` during background startup to tell the compositor not to manage the window.

## 📄 Logs

<details>
<summary>Click to expand logs</summary>

```
[Startup] Immediately hiding main window for background mode
[Startup] Background enforcer #1: window was visible, hiding again
[Startup] Background enforcer #2: window was visible, hiding again
...
```
</details>
