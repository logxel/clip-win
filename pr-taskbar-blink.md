## 📝 Description

When the app starts with `--background` (autostart to tray), a focus→hide→refocus loop between GNOME Mutter and the app's event handlers causes the taskbar icon to blink rapidly for 1–2 seconds.

The fix calls `set_skip_taskbar(true)` on the window during background-mode startup (with defense-in-depth in the enforcer thread and the `Focused(true)` handler), so the compositor doesn't manage the window's taskbar presence or attempt to auto-focus it. `set_skip_taskbar(false)` is restored on the first user toggle.

## 🔗 Related Issue

- **#139** (open) — Duplicate tray icons after suspend on GNOME. Different mechanism (tray icon vs taskbar icon, suspend vs startup), same environment. This PR does not close #139.

## 🧪 Type of Change

- [x] 🐛 Bug fix (non-breaking change that fixes an issue)
- [ ] ✨ New feature (non-breaking change that adds functionality)
- [ ] 💥 Breaking change (fix or feature that would cause existing functionality to change)
- [ ] 📚 Documentation update
- [ ] 🎨 Style/UI change
- [ ] ♻️ Refactoring (no functional changes)
- [ ] ⚡ Performance improvement
- [ ] 🧹 Chore (build process, dependencies, etc.)

## 📸 Screenshots

N/A

## ✅ Checklist

- [x] My code follows the project's code style
- [ ] I have run `make lint` and `make format`
- [x] I have tested my changes locally
- [ ] I have added/updated documentation as needed
- [x] My changes don't introduce new warnings
- [x] I have tested on both X11 and Wayland (if applicable)

## 🖥️ Testing Environment

- **OS**: Ubuntu 24.04
- **Desktop Environment**: GNOME 46
- **Display Server**: [X11 / Wayland]

## 📋 Additional Notes

- Branch: `fix/taskbar-blink-background-startup` (based on `upstream/master`)
- Single file changed: `src-tauri/src/main.rs` (+15 / −3)
- No behavior changes after the user has toggled the window at least once
- `set_skip_taskbar` is a no-op on compositors that don't support it
