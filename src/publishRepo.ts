// Pure derivations for publishing the open project to a new GitHub repo —
// kept out of the component so the whole vocabulary is unit-testable.
//
// The backend refuses a bad name, a non-root project and a missing `gh` too
// — this exists for friendlier messages without a round trip.

import type { RepoState, Visibility } from "./api";
import { parentName } from "./gitStatus";

export type { Visibility };
export type Check = { ok: true } | { ok: false; reason: string };

/** Default repo name from a project path — was inlined in Toolbar.tsx. */
export function defaultRepoName(sketchDir: string): string {
  return sketchDir.split("/").filter(Boolean).pop() ?? "";
}

/** GitHub's own rules: 1-100 chars, alphanumerics plus . - _ , not "." or "..". */
export function checkRepoName(name: string): Check {
  const n = name.trim();
  if (!n) return { ok: false, reason: "name the repository" };
  if (n.length > 100)
    return {
      ok: false,
      reason: `a repository name is 100 characters or fewer (got ${n.length})`,
    };
  if (n === "." || n === "..")
    return { ok: false, reason: `"${n}" is not a repository name` };
  const bad = [...n].find((c) => !/[A-Za-z0-9._-]/.test(c));
  if (bad !== undefined) {
    const hint = bad === " " ? " — use `-` instead of spaces" : "";
    return {
      ok: false,
      reason: `a repository name may only contain letters, digits, '_', '.' and '-' — found '${bad}'${hint}`,
    };
  }
  return { ok: true };
}

/** Why publishing is blocked right now, or null when it can proceed.
 *
 *  `no_git` is deliberately absent: the backend initializes the repository
 *  before creating the remote, so an untracked project can still publish.
 *
 *  Order: structural facts first, then the credential guard, and the missing
 *  `gh` last — its "paste a remote URL instead" advice is only true once
 *  nothing else blocks, since a pasted URL fixes none of the others. */
export function publishBlockedReason(
  state: RepoState | null,
  visibility: Visibility,
  ghAvailable: boolean,
): string | null {
  if (!state) return "open a project first";
  if (state.kind === "nested")
    return `the repository root is ${parentName(state.root)} — publish from there, not from this project`;
  if (state.kind === "root") {
    if (state.remote) return "this repository already has a remote — use Sync to push";
    if (visibility === "public" && state.tracked_secrets.length > 0) {
      const names = state.tracked_secrets.join(", ");
      const many = state.tracked_secrets.length > 1;
      return `${names} ${many ? "are" : "is"} tracked — a public repo would publish ${
        many ? "them" : "it"
      }; publish private, or untrack first`;
    }
  }
  if (!ghAvailable)
    return "the GitHub CLI (gh) is not installed — create the repo yourself and paste its remote URL";
  return null;
}
