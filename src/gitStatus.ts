// Pure derivations for the toolbar git pill — kept out of the component so
// the pill's whole vocabulary is unit-testable, like ports.ts is for ports.

import type { ChangedPath, RepoState } from "./api";

/** Last path segment, for "tracked by <parent>". */
export function parentName(root: string): string {
  const seg = root.split("/").filter(Boolean);
  return seg.length ? seg[seg.length - 1] : root;
}

/** The pill's text, or null when there is nothing to say (no open project). */
export function pillLabel(state: RepoState | null): string | null {
  if (!state) return null;
  switch (state.kind) {
    case "no_git":
      return "no git";
    case "nested":
      return `tracked by ${parentName(state.root)}`;
    case "root": {
      let label: string;
      if (state.detached) {
        label = state.dirty.length === 0 ? "detached" : `detached · ${state.dirty.length} changed`;
      } else {
        label = state.dirty.length === 0 ? "✓ clean" : `${state.dirty.length} changed`;
      }
      if (state.ahead > 0) label += ` ↑${state.ahead}`;
      if (state.behind > 0) label += ` ↓${state.behind}`;
      return label;
    }
  }
}

/** Mirrors core::git::suggested_message — `checkpoint: a, b (+N)`, the first
 *  two paths by name and the rest counted. Used to prefill the commit box
 *  for non-root states (nested), which carry a `dirty` list but no
 *  `suggested_message` of their own from core. */
export function suggestedMessage(dirty: ChangedPath[]): string {
  const names = dirty.map((c) => c.path);
  switch (names.length) {
    case 0:
      return "checkpoint";
    case 1:
      return `checkpoint: ${names[0]}`;
    case 2:
      return `checkpoint: ${names[0]}, ${names[1]}`;
    default:
      return `checkpoint: ${names[0]}, ${names[1]} (+${names.length - 2})`;
  }
}

export type PopoverMode = "actions" | "setup_remote" | "init" | "nested";

/** Which popover the pill opens. Remote setup replaces the actions until an
 *  origin exists — Commit still works there via the actions in that pane. */
export function popoverMode(state: RepoState): PopoverMode {
  switch (state.kind) {
    case "no_git":
      return "init";
    case "nested":
      return "nested";
    case "root":
      return state.remote ? "actions" : "setup_remote";
  }
}

/** Why flashes aren't being tagged in this repo state, or null when they are.
 *  A flash that changes the code writes a `flash/<timestamp>` tag, but only
 *  when the project dir *is* the repo root — same rule as Sync, and the same
 *  reason it must say why. Null with no project open: nothing to explain. */
export function flashTaggingNote(state: RepoState | null): string | null {
  if (!state) return null;
  switch (state.kind) {
    case "no_git":
      return "flashes aren't tagged — initialize a repository to record what's on the board";
    case "nested":
      return `flashes aren't tagged — the repository root is ${parentName(state.root)}, not this project`;
    case "root":
      return null;
  }
}

/** Why Sync is disabled right now, or null when it can run. The profile
 *  silently winning over the port taught us (2026-08-09) that disabled
 *  buttons must say why. */
export function syncDisabledReason(state: RepoState): string | null {
  if (state.kind !== "root") return "sync works from the repository root";
  if (state.detached) return "HEAD is detached — check out a branch first";
  if (state.dirty.length > 0) return "uncommitted changes — commit first";
  return null;
}
