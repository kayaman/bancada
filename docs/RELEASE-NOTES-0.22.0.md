# bancada 0.22.0

**Editing a bench project is safer and easier to operate from the keyboard.**
Bancada now protects work in progress when you change projects or close the
window, and the menus and panel dividers are usable without a mouse.

## Unsaved work

- Switching projects asks whether to Save, Discard, or Cancel before clearing
  editor buffers.
- Closing the desktop window uses the same decision, including a browser
  `beforeunload` fallback for preview builds.
- Verify, Flash, Commit, and Assistant sends stop when a save fails or newer
  edits arrived while the disk write was in progress.
- Overlapping saves are serialized, and a newer edit remains dirty instead of
  being removed by an older write completing later.

## Keyboard operation

- Menus focus their first enabled item when opened.
- Arrow keys, Home, End, Enter, and Escape work in menus; nested menus open
  with Right and close with Left while restoring the parent trigger.
- Sidebar and bottom-panel dividers expose their current size to assistive
  technology and resize with arrows, Shift+arrows, Home, End, and Enter.

## Under the hood

- Save sequencing and leave-project decisions live in a small pure TypeScript
  module with regression coverage.
- The Tauri close-request capability is explicitly granted to the main window.
- Frontend architecture documentation now describes the menu focus model.

## Tests

```
npm test       1313 passed, 82 files
npm run build  clean (Vite bundle; existing size warning remains)
cargo check -p bancada --locked --offline  clean
```

Hardware smoke tests remain manual and require an attached board and installed
toolchains; no board was connected for this release build.

The optimized Linux binary, DEB, and RPM bundles were produced successfully.
AppImage staging completed, but the linuxdeploy AppImage packaging step hangs
in this headless build environment; the AppImage should be regenerated on a
desktop release runner before distribution.
