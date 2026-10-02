//! Project CRUD and the bancada hand-off receiver, extracted from
//! enclosure-maker's own (retired) standalone `src-tauri` launcher so
//! bancada's Tauri app can call them directly in-process instead of
//! spawning a separate `enclosure-maker-app` binary. See
//! `enclosure-maker/README.md` for the sibling repo this was merged from.

pub mod bancada_import;
pub mod projects;
