import { describe, expect, it } from "vitest";
import { checkRepoName, defaultRepoName, publishBlockedReason } from "../publishRepo";
import type { RepoState } from "../api";

const root = (over: Partial<Extract<RepoState, { kind: "root" }>> = {}): RepoState => ({
  kind: "root",
  branch: "main",
  detached: false,
  dirty: [],
  remote: null,
  has_upstream: false,
  ahead: 0,
  behind: 0,
  tracked_secrets: [],
  suggested_message: "checkpoint",
  ...over,
});

describe("defaultRepoName", () => {
  it("is the sketch folder's name", () => {
    expect(defaultRepoName("/home/u/Projects/teste-uno-veia")).toBe("teste-uno-veia");
  });
  it("ignores trailing slashes", () => {
    expect(defaultRepoName("/home/u/Projects/Blink/")).toBe("Blink");
  });
  it("is empty for an empty path", () => {
    expect(defaultRepoName("")).toBe("");
    expect(defaultRepoName("/")).toBe("");
  });
});

describe("checkRepoName", () => {
  it("accepts GitHub's charset, trimmed", () => {
    expect(checkRepoName("teste-uno-veia")).toEqual({ ok: true });
    expect(checkRepoName("  my.repo_2  ")).toEqual({ ok: true });
    expect(checkRepoName(".dotfiles")).toEqual({ ok: true });
    expect(checkRepoName("a".repeat(100))).toEqual({ ok: true });
  });

  it("rejects an empty name", () => {
    const r = checkRepoName("   ");
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toBe("name the repository");
  });

  it("rejects over 100 characters, and says how many", () => {
    const r = checkRepoName("a".repeat(101));
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.reason).toContain("100 characters or fewer (got 101)");
  });

  it("rejects the two path-meaning names", () => {
    expect(checkRepoName(".").ok).toBe(false);
    expect(checkRepoName("..").ok).toBe(false);
  });

  it("names the offending character, with a hint for spaces", () => {
    const r = checkRepoName("my repo");
    expect(r.ok).toBe(false);
    if (!r.ok) {
      expect(r.reason).toContain("found ' '");
      expect(r.reason).toContain("instead of spaces");
    }
    const slash = checkRepoName("owner/repo");
    expect(slash.ok).toBe(false);
    if (!slash.ok) expect(slash.reason).toContain("found '/'");
  });
});

describe("publishBlockedReason", () => {
  it("blocks with no sketch open", () => {
    expect(publishBlockedReason(null, "private", true)).toBe("open a sketch first");
  });

  it("does not block an untracked sketch — the backend inits first", () => {
    expect(publishBlockedReason({ kind: "no_git" }, "private", true)).toBeNull();
    expect(publishBlockedReason({ kind: "no_git" }, "public", true)).toBeNull();
  });

  it("passes a clean root repo with no remote", () => {
    expect(publishBlockedReason(root(), "private", true)).toBeNull();
    expect(publishBlockedReason(root(), "public", true)).toBeNull();
  });

  it("blocks a repo that already has a remote", () => {
    const r = publishBlockedReason(root({ remote: "git@github.com:m/x.git" }), "private", true);
    expect(r).toMatch(/already has a remote/i);
  });

  it("blocks a nested sketch, naming the root — gh repo create --source would publish the wrong tree", () => {
    const r = publishBlockedReason(
      { kind: "nested", root: "/home/u/Projects", dirty: [] },
      "private",
      true,
    );
    expect(r).toContain("Projects");
    expect(r).toMatch(/publish from there/i);
  });

  it("hard-blocks a public repo with tracked credentials, naming the files", () => {
    const state = root({ tracked_secrets: ["secrets.h"] });
    const r = publishBlockedReason(state, "public", true);
    expect(r).toContain("secrets.h");
    expect(r).toContain("is tracked");
    expect(publishBlockedReason(state, "private", true)).toBeNull();
  });

  it("lists every tracked credential, plural", () => {
    const r = publishBlockedReason(
      root({ tracked_secrets: ["secrets.h", "web/.env"] }),
      "public",
      true,
    );
    expect(r).toContain("secrets.h, web/.env");
    expect(r).toContain("are tracked");
  });

  it("blocks without gh, pointing at the paste-a-URL path", () => {
    const r = publishBlockedReason(root(), "private", false);
    expect(r).toMatch(/gh/);
    expect(r).toMatch(/remote URL/i);
    expect(publishBlockedReason({ kind: "no_git" }, "private", false)).toMatch(/gh/);
  });

  it("reports the secrets block before the missing gh — a pasted URL fixes neither", () => {
    const r = publishBlockedReason(root({ tracked_secrets: ["secrets.h"] }), "public", false);
    expect(r).toContain("secrets.h");
  });
});
