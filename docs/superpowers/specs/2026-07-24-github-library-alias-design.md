# Design: reference a GitHub library by alias, pinned to a version

## Problem

Bancada can pin a registry library (`ArduinoJson (7.4.2)`) and can point at a
local folder (`dir: ../libs/Foo`), but it has no way to say *"this library comes
from that GitHub repo, at that version."*

The user's own libraries live in `github.com/kayaman/Arduino` under
`libraries/{HomeNode,StatusLed,StatusPanel}`, and today the workflow is manual:
clone the repo to `~/Projects/Arduino`, then symlink individual libraries into
the sketchbook (`~/Arduino/libraries/HomeNode -> ../../Projects/Arduino/libraries/HomeNode`).
Nothing records *which version* a given sketch was built against.

Goal: an alias like `@kayaman/Arduino/libraries/HomeNode`, resolved to a pinned
version, reproducible on another machine.

## Constraints discovered (verified, not assumed)

1. **`sketch.yaml` has no git form.** A profile's `libraries:` accepts exactly
   three shapes: `Name (version)`, `dependency: Name (version)`, and
   `dir: <path>`. There is no URL/git entry, and **`dir:` carries no version**.
   → Bancada must resolve the alias itself and record the pin itself.

2. **arduino-cli's own git support is unusable here.** `lib install --git-url`
   fails with *"--git-url and --zip-path are disabled by default"* — it needs a
   global `library.enable_unsafe_install` config change that weakens the user's
   arduino-cli posture. Even enabled, it installs into the **sketchbook**, which
   profile builds exclude, so a `dir:` entry would still be required. Rejected on
   both counts.

3. **`git ls-remote --tags <url>` lists tags with SHAs and needs no GitHub
   token.** Confirmed against the real repo. This removes the GitHub REST API,
   its auth handling, and its rate limits from the design entirely, and makes the
   feature work for any git host, not just GitHub.

4. **Annotated tags resolve to two SHAs.** `ls-remote` emits both the tag object
   and a `^{}` row for the commit it dereferences to:
   ```
   edebdae…  refs/tags/HomeNode/v1.1.0
   ef11f73…  refs/tags/HomeNode/v1.1.0^{}
   ```
   The pin must record the **commit** (`^{}`), not the tag object.

5. **Tags are namespaced per library**: `HomeNode/v1.1.0`, `StatusLed/v1.0.0`.
   The namespace matches the library's directory name, which makes filtering the
   relevant versions for a given alias straightforward.

6. A fetched library looks like one: `libraries/HomeNode` contains
   `library.properties`, `src/`, `examples/`, `keywords.txt`. That gives a cheap
   correctness check on the resolved path.

## Alias syntax

```
@<owner>/<repo>[/<path…>][@<ref>]
```

- `@kayaman/Arduino/libraries/HomeNode` — path inside the repo
- `@kayaman/HomeNode` — library at the repo root (path empty)
- `@kayaman/Arduino/libraries/HomeNode@HomeNode/v1.1.0` — explicit ref

Parsed by stripping the leading `@`, splitting the ref off at the **last** `@`
(the ref itself may contain `/`), then splitting the remainder on `/`: first
segment owner, second repo, remainder the in-repo path. The library's name is the
last path segment, or the repo name when the path is empty.

The UI does not require the `@<ref>` suffix — version selection is interactive.
The suffix exists so an alias is a complete, scriptable identifier.

## Resolution flow

1. **List versions** — `git ls-remote --tags <https url>`. Keep only `^{}` rows
   (commits), strip `refs/tags/`. Partition into tags whose first path segment
   equals the library name (`HomeNode/*`) and the rest; present the former first,
   newest first, with all tags available as a fallback for repos that don't use
   the convention. Sort by parsing the segment after the last `/`, with an
   optional leading `v`, as a dotted numeric version; anything unparseable sorts
   last, lexicographically, rather than being hidden.
2. **User confirms** a tag (newest matching preselected).
3. **Fetch** into a temp dir:
   ```
   git clone --depth 1 --filter=blob:none --sparse --branch <ref> <url> <tmp>
   git -C <tmp> sparse-checkout set <path>
   ```
   Then assert `HEAD` equals the commit `ls-remote` reported for that tag. A
   mismatch means the tag moved between listing and fetching — refuse rather than
   silently build something else.
4. **Validate** `<tmp>/<path>/library.properties` exists. Absent ⇒ the path is
   wrong (or the repo layout changed); refuse with the path we looked at.
5. **Materialise** by copying `<tmp>/<path>` into
   `<sketch>/.bancada/libs/<LibName>`, built in a dot-prefixed staging directory
   and moved with one rename, then delete the temp clone. Copying the subtree
   rather than referencing it inside the clone is deliberate: it keeps `.git` out
   of the vendored copy, which would otherwise be a nested repository inside the
   user's own sketch repo.
6. **Pin in the profile** — `dir: .bancada/libs/<LibName>`, **relative**, via the
   existing `SketchProject::add_local_library` (whose `PathStyle::Relative`
   default and resolved-target dedup already do the right thing).
7. **Record the pin** in `<sketch>/bancada.yaml`.
8. **Offer** to add `.bancada/` to the sketch's `.gitignore`.

## The manifest

`sketch.yaml` belongs to arduino-cli; Bancada does not extend its schema. The pin
lives in a sibling file, `<sketch>/bancada.yaml`:

```yaml
version: 1
libraries:
  - alias: "@kayaman/Arduino/libraries/HomeNode"
    ref: HomeNode/v1.1.0
    commit: ef11f7390389bf15a1002ea8eb82a2662713257
    vendor: .bancada/libs/HomeNode
```

`ref` is what the user chose; `commit` is what it resolved to. Recording both is
what makes "pinned" mean something: a tag can be moved, a commit cannot.

`bancada.yaml` is committed; `.bancada/` is gitignored. A fresh clone therefore
carries the pins but not the bytes, and **restore** re-materialises them at the
recorded commit. Vendoring the bytes instead would work offline but bloat the
repo, and is not the default.

## Operations in scope

- **Add** — the flow above.
- **Restore** — for every manifest entry, re-fetch at the recorded `commit` and
  re-materialise. This is what makes a fresh clone buildable, and is the reason
  the manifest exists at all.

Re-adding an alias with a newer tag *is* the update path, so no separate "update"
operation is needed. Deliberately out of scope: private-repo credential
management (git handles it), non-GitHub hosts (the syntax is GitHub-shaped though
the mechanism is not), and vendor-and-commit mode.

## Structure

`core/src/ghlib.rs` — new. Pure and unit-tested:

- `parse_alias(&str) -> Result<GhAlias>` (owner, repo, path, name, optional ref)
- `alias_url(&GhAlias) -> String`
- `parse_ls_remote(&str) -> Vec<RemoteTag>` — `^{}` handling lives here
- `rank_versions(&[RemoteTag], lib_name) -> Vec<RemoteTag>` — filter + sort
- `Manifest` model with `load`/`save`, `upsert(entry)`
- `vendor_rel_path(name) -> String`

plus thin git-invoking functions (`list_remote_tags`, `fetch_subtree`) — shelling
out to a tool matches what `cli.rs` already does for arduino-cli. The atomic
staging pattern is lifted from `library.rs::create_library`.

`src-tauri` gains `gh_list_versions`, `gh_add_library`, `gh_restore`, all
`async` + `spawn_blocking`, following the existing library-command convention.
`git` becomes a documented prerequisite alongside `arduino-cli` and `esptool`.

UI: a fourth **GitHub** tab in `LibraryManager` — alias input, a version select
populated by `gh_list_versions`, an Add button, and a list of current manifest
entries each showing its ref and short commit, with a Restore action. No modal:
the app still has no dialog primitive and this does not introduce the first one.

## Error handling

Every failure names what it looked at, and nothing is left half-written:

| Failure | Behaviour |
|---|---|
| malformed alias | refuse, showing the expected shape |
| `git` missing | refuse, naming git as a prerequisite |
| repo unreachable / private without credentials | surface git's own stderr |
| no tags at all | offer the default branch as an unpinned fallback, clearly labelled |
| tag moved between list and fetch | refuse, reporting both SHAs |
| no `library.properties` at the path | refuse, naming the path inspected |
| vendor dir already exists | overwrite, since re-adding is the update path (unlike `create_library`, where refusing is right) |
| profile edit fails | non-fatal — the library is vendored and the manifest written; report it, as `create_library` does |

## Testing

Pure unit tests in `core/src/ghlib.rs` with no network: alias parsing (with and
without a path, with a ref containing `/`, malformed inputs); `ls-remote` parsing
against captured real output including the `^{}` pairs; version ranking
(namespaced vs unrelated tags, `v` prefix, unparseable tags sorting last);
manifest round-trip and `upsert` replacing an existing alias.

One `#[ignore]`d integration test that really fetches
`@kayaman/Arduino/libraries/HomeNode` at `HomeNode/v1.1.0`, asserts the vendored
tree has `library.properties` and no `.git`, and compiles a sketch against it —
mirroring `core/tests/scaffold_compiles.rs`.

## Known consequences

- **The vendored tree is a copy.** Editing it edits a build input that `restore`
  will overwrite. Local development of a library still belongs in a real clone
  plus `+ Local…`.
- **Restore needs network** on a fresh clone. That is the accepted cost of not
  committing vendored bytes.
- `save_yaml` still reflows `sketch.yaml` (comments dropped, profiles reordered).
  Pre-existing; this adds a fourth trigger.
