// Pure derivations for the toolbar git pill — kept out of the component so
// the pill's whole vocabulary is unit-testable, like ports.ts is for ports.

import type { RepoState } from "./api";

/** Last path segment, for "tracked by <parent>". */
export function parentName(root: string): string {
  const seg = root.split("/").filter(Boolean);
  return seg.length ? seg[seg.length - 1] : root;
}

/** The pill's text, or null when there is nothing to say (no open sketch). */
export function pillLabel(state: RepoState | null): string | null {
  if (!state) return null;
  switch (state.kind) {
    case "no_git":
      return "no git";
    case "nested":
      return `tracked by ${parentName(state.root)}`;
    case "root": {
      let label = state.dirty.length === 0 ? "✓ clean" : `${state.dirty.length} changed`;
      if (state.ahead > 0) label += ` ↑${state.ahead}`;
      if (state.behind > 0) label += ` ↓${state.behind}`;
      return label;
    }
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

/** Why Sync is disabled right now, or null when it can run. The profile
 *  silently winning over the port taught us (2026-08-09) that disabled
 *  buttons must say why. */
export function syncDisabledReason(state: RepoState): string | null {
  if (state.kind !== "root") return "sync works from the repository root";
  if (state.detached) return "HEAD is detached — check out a branch first";
  if (state.dirty.length > 0) return "uncommitted changes — commit first";
  return null;
}
