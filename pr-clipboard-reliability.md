## 📝 Description

Three clipboard reliability fixes for Linux:

1. **Prevent orphaned child processes** — Track `xclip`/`wl-copy` children in a `Mutex<Option<Child>>` and kill/reap them on replacement or `Drop`. Previously every copy spawned a process that was never cleaned up.
2. **Fix clipboard data loss on X11** — Add `-loops 0` to all `xclip` invocations so `xclip` stays alive to serve paste requests instead of exiting immediately.
3. **Add Wayland clipboard read fallback** — When `arboard` fails to read the clipboard on Wayland, fall back to `wl-paste --no-newline`.

Also adds a comprehensive test suite (19 tests: 14 pure-logic + 5 integration, using polling instead of fixed sleeps).

## 🔗 Related Issue

- **#259** (open) — Same root cause as fix #2 (clipboard owner exits prematurely), but for images via `arboard`. This PR does not close #259.

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

- Branch: `consolidated-clipboard-fixes` (based on `upstream/master`)
- Single file changed: `src-tauri/src/clipboard_manager.rs` (+458 / −12)
- All tests pass: `cargo test --lib clipboard_manager` → 19 passed, 0 failed
- Temp file names use `clip-hist-test-` prefix (neutral naming)
- No public API or config changes
