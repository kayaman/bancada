// Realistic VS Code theme shapes, kept as fixtures because the interesting
// cases are all about what a theme LEAVES OUT.

import type { VsCodeTheme } from "../../vscode";

/** Keys lifted from VS Code's own Dark+ — a well-populated theme. */
export const DARK_PLUS: VsCodeTheme = {
  name: "Dark+ (test subset)",
  type: "dark",
  colors: {
    "editor.background": "#1f1f1f",
    "editor.foreground": "#cccccc",
    foreground: "#cccccc",
    "sideBar.background": "#181818",
    "panel.background": "#181818",
    "dropdown.background": "#313131",
    "input.background": "#313131",
    "list.hoverBackground": "#2a2d2e",
    "panel.border": "#2b2b2b",
    "input.border": "#3c3c3c",
    focusBorder: "#0078d4",
    "button.background": "#0078d4",
    "button.foreground": "#ffffff",
    descriptionForeground: "#9d9d9d",
    "editorWarning.foreground": "#cca700",
    "editorError.foreground": "#f85149",
    "gitDecoration.addedResourceForeground": "#2ea043",
  },
};

/** The other common shape: a theme that defines almost nothing and expects
 *  the editor to fill in. Everything but bg/fg must be derived. */
export const SPARSE: VsCodeTheme = {
  name: "Sparse",
  type: "dark",
  colors: {
    "editor.background": "#101014",
    "editor.foreground": "#e0e0e6",
  },
};

/** Light, and leaning on 8-digit hex — the idiom that breaks a naive parser.
 *  `#0000000a` over white is a very pale grey, not near-black. */
export const LIGHT_ALPHA: VsCodeTheme = {
  name: "Paper",
  type: "light",
  colors: {
    "editor.background": "#ffffff",
    "editor.foreground": "#24292f",
    "sideBar.background": "#f6f8fa",
    "list.hoverBackground": "#0000000a",
    "panel.border": "#d0d7de1a",
    focusBorder: "#0969da",
    "button.background": "#1f883d",
    "button.foreground": "#ffffff",
  },
};

/** Deliberately unreadable: the kind of thing that looks striking in a
 *  screenshot and cannot be used to read a compiler error. Every foreground
 *  is a hair away from its background. */
export const AWFUL: VsCodeTheme = {
  name: "Midnight Whisper",
  type: "dark",
  colors: {
    "editor.background": "#0a0a0f",
    "editor.foreground": "#1a1a24",
    "sideBar.background": "#0c0c12",
    "panel.border": "#0d0d14",
    "input.border": "#0e0e15",
    descriptionForeground: "#121219",
    focusBorder: "#141420",
    "button.background": "#101018",
    "button.foreground": "#131320",
    "editorWarning.foreground": "#15151f",
    "editorError.foreground": "#161622",
    "gitDecoration.addedResourceForeground": "#171724",
  },
};

/** A theme whose declared `type` disagrees with its actual background. */
export const MISLABELLED: VsCodeTheme = {
  name: "Mislabelled",
  type: "dark",
  colors: {
    "editor.background": "#fdfdfd",
    "editor.foreground": "#202020",
  },
};
