import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import BoardPicker from "./BoardPicker";
import {
  boardCandidates,
  boardCatalog,
  createIdfProject,
  createProject,
  defaultProjectParent,
  knownIdfTargets,
  listAllBoards,
  listIdfTemplates,
  listSketchTemplates,
  loadSettings,
  searchLibraries,
  setLastProjectParent,
  type Board,
  type BoardOption,
  type IdfTemplate,
  type IndexedLibrary,
  type NewProjectPlatform,
  type SketchTemplate,
} from "../api";

interface Props {
  /** FQBN of the board currently attached, preselected when known. */
  detectedFqbn: string | null;
  onCreated: (dir: string) => void;
  onCancel: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

/** `esp32:esp32:esp32s3:opts` -> `esp32s3`; mirrors project.rs for the preview. */
const profileFor = (fqbn: string) => fqbn.split(":")[2] ?? "";

export default function NewProject({
  detectedFqbn,
  onCreated,
  onCancel,
  notify,
}: Props) {
  const [name, setName] = useState("");
  const [parent, setParent] = useState("");
  const [boards, setBoards] = useState<BoardOption[]>([]);
  const [fqbn, setFqbn] = useState(detectedFqbn ?? "");
  const [profile, setProfile] = useState("");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<IndexedLibrary[]>([]);
  const [picked, setPicked] = useState<Record<string, string>>({});
  const [templates, setTemplates] = useState<SketchTemplate[]>([]);
  const [template, setTemplate] = useState("blink");
  // Which toolchain this project will be built with. Chosen, not detected —
  // `ProjectKind` answers that for a directory that already exists, and this
  // is the decision made before one does.
  const [platform, setPlatform] = useState<NewProjectPlatform>("arduino");
  const [idfTemplates, setIdfTemplates] = useState<IdfTemplate[]>([]);
  const [idfTemplate, setIdfTemplate] = useState("hello");
  const [idfTargets, setIdfTargets] = useState<string[]>([]);
  const [idfTarget, setIdfTarget] = useState("");
  /** Every modelled devkit, for filtering by target on the ESP-IDF side. */
  const [allBoards, setAllBoards] = useState<Board[]>([]);
  const [arduinoDevkits, setArduinoDevkits] = useState<Board[]>([]);
  // "" means "not listed" — an explicit choice, not an unanswered question.
  // The project is then created with no board recorded, and `project_board`
  // infers one from the FQBN if it can, labelled as the guess it is.
  const [board, setBoard] = useState("");
  const [working, setWorking] = useState(false);

  // Default the location to wherever the last project went, falling back to
  // ~/Projects — or the home directory itself for users who don't keep one.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      const [settings, fallback] = await Promise.all([
        loadSettings().catch(() => ({}) as Awaited<ReturnType<typeof loadSettings>>),
        defaultProjectParent().catch(() => ""),
      ]);
      if (cancelled) return;
      setParent(settings.last_new_project_parent || fallback);
    })();
    listAllBoards()
      .then((b) => {
        if (!cancelled) setBoards(b);
      })
      .catch((e) => notify(String(e), true));
    // A missing template list degrades to the backend's Blink default —
    // creation must not depend on this call succeeding.
    listSketchTemplates()
      .then((t) => {
        if (!cancelled) setTemplates(t);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The devkits Bancada carries pin data for on the chosen board's chip.
  // Resolved in Rust — the FQBN→chip fold is not something the UI should own
  // a second copy of. A lone candidate is preselected: with one option, asking
  // would be a question whose answer is foregone, and the user can still say
  // "not listed". A failure here leaves the list empty, which the form treats
  // exactly like a chip with no modelled devkit — creation never depends on it.
  useEffect(() => {
    if (!fqbn) {
      setArduinoDevkits([]);
      return;
    }
    let cancelled = false;
    boardCandidates(fqbn)
      .then((bs) => {
        if (!cancelled) setArduinoDevkits(bs);
      })
      .catch(() => {
        if (!cancelled) setArduinoDevkits([]);
      });
    return () => {
      cancelled = true;
    };
  }, [fqbn]);

  /** The chip the attached board runs, when Bancada can name it. Derived from
   *  the Arduino candidate list — those boards were resolved through the FQBN
   *  fold in Rust, so this reuses that answer rather than re-folding here. */
  const detectedTarget = arduinoDevkits[0]?.target ?? "";

  // The ESP-IDF half of the form. All three calls are pure reads of core's own
  // tables — deliberately *not* `idf.py --list-targets`, because creating a
  // project must not require a working install. A missing toolchain is the
  // first build's problem to report, not the wizard's.
  useEffect(() => {
    if (platform !== "idf") return;
    let cancelled = false;
    Promise.all([
      listIdfTemplates().catch(() => [] as IdfTemplate[]),
      knownIdfTargets().catch(() => [] as string[]),
      boardCatalog().then((c) => c.boards).catch(() => [] as Board[]),
    ]).then(([tmpls, targets, boards]) => {
      if (cancelled) return;
      setIdfTemplates(tmpls);
      setIdfTargets(targets);
      setAllBoards(boards);
      // Preselect the chip of the attached board when we can name it, so the
      // common case — a board is plugged in — needs no choice at all.
      setIdfTarget((t) => t || detectedTarget || targets[0] || "");
    });
    return () => {
      cancelled = true;
    };
  }, [platform, detectedTarget]);

  // The devkits offered, from whichever side of the form is showing. On the
  // ESP-IDF side the chip is already in hand, so this is a filter rather than
  // the FQBN fold `board_candidates` performs.
  const devkits =
    platform === "idf"
      ? allBoards.filter((b) => b.target === idfTarget)
      : arduinoDevkits;

  // A lone candidate is preselected: with one option, asking would be a
  // question whose answer is foregone. Runs on every change of the list, so
  // switching platform or target re-decides rather than stranding a board
  // belonging to the other chip.
  useEffect(() => {
    setBoard(devkits.length === 1 ? devkits[0].id : "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [devkits.map((b) => b.id).join(",")]);

  const effectiveProfile = profile.trim() || profileFor(fqbn);
  const dest = parent && name.trim() ? `${parent}/${name.trim()}` : "";

  const chooseParent = async () => {
    const dir = await open({ directory: true, title: "Where to create the project" });
    if (typeof dir !== "string") return;
    setParent(dir);
  };

  const doSearch = async () => {
    if (!query.trim()) return;
    setWorking(true);
    try {
      setResults(await searchLibraries(query.trim()));
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const toggle = (lib: IndexedLibrary) =>
    setPicked((p) => {
      const next = { ...p };
      if (next[lib.name]) delete next[lib.name];
      else next[lib.name] = lib.latest.version;
      return next;
    });

  const create = async () => {
    // `working` guards the same hazard DuplicateProject and RenameProject
    // guard: Enter in the name field bypasses the disabled button, so two
    // quick presses would fire two createProject calls at the same path.
    if (!canCreate) return;
    setWorking(true);
    try {
      if (platform === "idf") {
        const res = await createIdfProject(
          parent,
          name.trim(),
          idfTemplate,
          idfTarget || null,
          board || null,
        );
        setLastProjectParent(parent).catch(() => {});
        // Same non-fatal boundary as the Arduino side: the project exists and
        // builds without git, so name what fell short rather than hiding it
        // behind a plain success.
        notify(
          res.git_error
            ? `Created ${res.dir}, but not put under git: ${res.git_error}`
            : `✓ Created ${res.dir} (ESP-IDF${res.target ? `, ${res.target}` : ""})`,
          Boolean(res.git_error),
        );
        onCreated(res.dir);
        return;
      }
      // Pin explicitly rather than leaving versions floating.
      const libraries = Object.entries(picked).map(([n, v]) => `${n}@${v}`);
      const res = await createProject(
        parent,
        name.trim(),
        fqbn,
        profile.trim() || null,
        libraries,
        template,
        board || null,
      );
      // Remembering the parent is a convenience; never fail creation over it.
      setLastProjectParent(parent).catch(() => {});
      // Both are non-fatal: the sketch exists and builds either way, so say
      // what fell short rather than hiding it behind a plain success.
      const warnings: string[] = [];
      if (res.library_errors.length) {
        warnings.push(
          `${res.library_errors.length} librar${res.library_errors.length === 1 ? "y" : "ies"} failed: ${res.library_errors.join("; ")}`,
        );
      }
      if (res.git_error) {
        warnings.push(`not put under git: ${res.git_error}`);
      }
      if (res.board_error) {
        warnings.push(`board not recorded: ${res.board_error}`);
      }
      notify(
        warnings.length
          ? `Created ${res.dir}, but ${warnings.join("; ")}`
          : `✓ Created ${res.dir} (profile ${res.profile})`,
        warnings.length > 0,
      );
      onCreated(res.dir);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const pickedCount = Object.keys(picked).length;

  // ESP-IDF needs no board platform installed, so it must not inherit the
  // Arduino side's FQBN requirement — that gate would make the whole point of
  // the ESP-IDF path (a project without arduino-cli's world) unreachable.
  const canCreate =
    !working &&
    Boolean(name.trim()) &&
    Boolean(parent) &&
    (platform === "idf" ? Boolean(idfTarget) : Boolean(fqbn));

  return (
    <div className="new-project">
      <div className="np-head">
        <strong>New project</strong>
        <div className="spacer" />
        <button className="btn small" onClick={onCancel} disabled={working}>
          Cancel
        </button>
        <button
          className="btn small primary"
          onClick={create}
          disabled={!canCreate}
        >
          Create project
        </button>
      </div>

      <div className="np-body">
        <div className="field">
          Platform
          <div className="np-platform" role="radiogroup" aria-label="Platform">
            {(
              [
                ["arduino", "Arduino", "arduino-cli · a sketch with a pinned profile"],
                ["idf", "ESP-IDF", "idf.py · a CMake project with a main component"],
              ] as const
            ).map(([id, label, hint]) => (
              <button
                key={id}
                type="button"
                role="radio"
                aria-checked={platform === id}
                className={`np-tmpl-card${platform === id ? " selected" : ""}`}
                onClick={() => setPlatform(id)}
                disabled={working}
              >
                <span className="np-tmpl-label">{label}</span>
                <span className="np-tmpl-desc">{hint}</span>
              </button>
            ))}
          </div>
        </div>

        <label className="field">
          Name
          <input
            className="input"
            placeholder={platform === "idf" ? "blink_node" : "BlinkNode"}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && create()}
          />
        </label>
        {platform === "idf" && (
          <div className="scope-dim">
            becomes <code>project({name.trim() || "name"})</code> — letters,
            digits, <code>_</code> and <code>-</code>, not starting with a digit
          </div>
        )}

        <label className="field">
          Location
          <span className="np-row">
            <input
              className="input"
              value={parent}
              onChange={(e) => setParent(e.target.value)}
            />
            <button className="btn small" onClick={chooseParent} disabled={working}>
              Choose…
            </button>
          </span>
        </label>

        {dest && <div className="lib-dest">{dest}</div>}

        {platform === "arduino" && (
          <>
            <label className="field">
              Board
              <BoardPicker
                boards={boards}
                value={fqbn}
                onChange={setFqbn}
                title="Board for the project's profile"
              />
            </label>
            {detectedFqbn && fqbn === detectedFqbn && (
              <div className="scope-dim">preselected from the attached board</div>
            )}
            {boards.length === 0 && (
              <div className="empty-hint">
                No installed platforms found — install a core first (a board
                platform is required, because the profile pins its version).
              </div>
            )}
          </>
        )}

        {platform === "idf" && (
          <>
            <label className="field">
              Target chip
              <select
                className="select"
                value={idfTarget}
                onChange={(e) => setIdfTarget(e.target.value)}
                disabled={working}
                title="Written to sdkconfig.defaults as CONFIG_IDF_TARGET"
              >
                {idfTargets.map((t) => (
                  <option key={t} value={t}>
                    {t}
                  </option>
                ))}
              </select>
            </label>
            <div className="scope-dim">
              written to <code>sdkconfig.defaults</code> as{" "}
              <code>CONFIG_IDF_TARGET</code> — the first build picks it up. No
              ESP-IDF install is needed to create the project, only to build it.
            </div>
          </>
        )}

        {/* Only rendered when there is something to choose. A chip with no
            modelled devkit gets the one-line note below instead of an empty
            control — a select with nothing in it reads as a bug. */}
        {devkits.length > 0 && (
          <label className="field">
            Devkit
            <select
              className="select"
              value={board}
              onChange={(e) => setBoard(e.target.value)}
              disabled={working}
              title="Which board this chip is on — decides the LED pin and the pin-safety warnings"
            >
              {devkits.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.name}
                </option>
              ))}
              <option value="">Not listed</option>
            </select>
          </label>
        )}
        {devkits.length > 0 && board && (
          <div className="scope-dim">
            pin warnings and the onboard LED come from this board's pinout
          </div>
        )}
        {devkits.length > 0 && !board && (
          <div className="scope-dim">
            no board recorded — pin warnings will be unavailable until one is set
          </div>
        )}
        {(platform === "idf" ? Boolean(idfTarget) : Boolean(fqbn)) &&
          devkits.length === 0 && (
            <div className="scope-dim">
              Bancada carries no pinout for this chip — the project builds
              normally, there are simply no pin warnings for it
            </div>
          )}

        {platform === "idf" && idfTemplates.length > 0 && (
          <div className="field">
            Starter
            <div className="np-tmpl-cards" role="radiogroup" aria-label="Starter template">
              {idfTemplates.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  role="radio"
                  aria-checked={idfTemplate === t.id}
                  className={`np-tmpl-card${idfTemplate === t.id ? " selected" : ""}`}
                  onClick={() => setIdfTemplate(t.id)}
                  disabled={working}
                >
                  <span className="np-tmpl-label">{t.label}</span>
                  <span className="np-tmpl-desc">{t.description}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {platform === "arduino" && templates.length > 0 && (
          <div className="field">
            Starter
            <div className="np-tmpl-cards" role="radiogroup" aria-label="Starter template">
              {templates.map((t) => (
                <button
                  key={t.id}
                  type="button"
                  role="radio"
                  aria-checked={template === t.id}
                  className={`np-tmpl-card${template === t.id ? " selected" : ""}`}
                  onClick={() => setTemplate(t.id)}
                  disabled={working}
                >
                  <span className="np-tmpl-label">{t.label}</span>
                  <span className="np-tmpl-desc">{t.description}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Profiles and the registry are arduino-cli's world. An ESP-IDF
            project has neither: its dependencies are components, resolved by
            the IDF Component Manager from its own registry, which is a
            different subsystem and deliberately out of scope here. Rendering
            these disabled would be a lie about what the form can do. */}
        {platform === "arduino" && (
          <>
            <label className="field">
              Profile name
              <input
                className="input"
                placeholder={profileFor(fqbn) || "derived from the board"}
                value={profile}
                onChange={(e) => setProfile(e.target.value)}
              />
            </label>
            {fqbn && (
              <div className="scope-dim">
                sketch.yaml will pin <code>{fqbn}</code> as profile{" "}
                <code>{effectiveProfile}</code>, with the installed platform
                version
              </div>
            )}
          </>
        )}

        {platform === "arduino" && (
        <div className="np-libs">
          <div className="np-row">
            <input
              className="input"
              placeholder="search the registry to add libraries…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && doSearch()}
            />
            <button className="btn small" onClick={doSearch} disabled={working}>
              Search
            </button>
            {pickedCount > 0 && (
              <span className="scope-dim">{pickedCount} selected</span>
            )}
          </div>
          <div className="np-lib-list">
            {results.map((lib) => (
              <label key={lib.name} className="np-lib">
                <input
                  type="checkbox"
                  checked={Boolean(picked[lib.name])}
                  onChange={() => toggle(lib)}
                />
                <span className="lib-name">{lib.name}</span>
                <span className="lib-version">{lib.latest.version}</span>
                <span className="lib-sentence">{lib.latest.sentence}</span>
              </label>
            ))}
            {results.length === 0 && (
              <div className="empty-hint">
                Optional. Selected libraries are pinned into the profile with
                their dependencies resolved.
              </div>
            )}
          </div>
        </div>
        )}
      </div>
    </div>
  );
}
