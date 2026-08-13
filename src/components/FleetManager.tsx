import { useEffect, useState } from "react";
import {
  fleetSync,
  forgetBoard,
  identifyBoard,
  projectDrift,
  setBoardNickname,
  type DetectedPort,
  type FleetEntry,
  type ProjectDrift,
} from "../api";
import { driftLabel, projectName } from "../boardOffer";
import { fleetDisplayName, portTitle } from "../ports";

interface Props {
  /** The current port scan; drives which boards show as online. */
  ports: DetectedPort[];
  /** Called before esptool runs so the build console can be shown. */
  onStreamStart: () => void;
  /** The project open right now — a board pointing at it needs no offer. */
  openSketchDir: string | null;
  /** Open a project directory. Discards unsaved buffers, hence `openCost`. */
  onOpenProject: (dir: string) => void;
  /**
   * What opening a project right now would destroy, or null. Checked at the
   * moment of the click rather than passed as a boolean, so a file dirtied
   * after the card was selected still counts.
   */
  openCost: () => string | null;
  notify: (msg: string, isError?: boolean) => void;
}

/** Epoch seconds → a compact local stamp, matching ScopeView's export format. */
const when = (secs: number) => {
  if (!secs) return "—";
  const d = new Date(secs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
};

/** "3 days ago" is more useful than a date for the last-seen column. */
const ago = (secs: number, now: number) => {
  if (!secs) return "never";
  const d = Math.max(0, now - secs);
  if (d < 90) return "just now";
  if (d < 3600) return `${Math.round(d / 60)} min ago`;
  if (d < 86400) return `${Math.round(d / 3600)} h ago`;
  return `${Math.round(d / 86400)} d ago`;
};

export default function FleetManager({
  ports,
  onStreamStart,
  openSketchDir,
  onOpenProject,
  openCost,
  notify,
}: Props) {
  const [boards, setBoards] = useState<FleetEntry[]>([]);
  const [online, setOnline] = useState<string[]>([]);
  const [unidentified, setUnidentified] = useState<DetectedPort[]>([]);
  const [working, setWorking] = useState(false);
  /** Id whose nickname is being edited, and the draft text. */
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [confirmForget, setConfirmForget] = useState<string | null>(null);
  const [nowSecs, setNowSecs] = useState(() => Math.floor(Date.now() / 1000));
  /** The expanded card. Selection is what triggers the drift lookup, so it is
   *  one `git rev-list` per click rather than one per board per scan. */
  const [selected, setSelected] = useState<string | null>(null);
  const [drift, setDrift] = useState<ProjectDrift | null>(null);
  /** Board id whose Open is armed, when opening would discard something. */
  const [armed, setArmed] = useState<string | null>(null);

  const apply = (s: {
    boards: FleetEntry[];
    online: string[];
    unidentified: DetectedPort[];
  }) => {
    setBoards(s.boards);
    setOnline(s.online);
    setUnidentified(s.unidentified);
  };

  /** Sync on mount and whenever the port scan changes — that is what enrols a
   *  newly plugged board. */
  useEffect(() => {
    setNowSecs(Math.floor(Date.now() / 1000));
    fleetSync(ports)
      .then(apply)
      .catch((e) => notify(String(e), true));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ports]);

  const isOnline = (id: string) => online.includes(id);

  const selectedRec = boards.find((b) => b.id === selected)?.last_flash ?? null;

  /**
   * Fetch drift for the selected board's flash record.
   *
   * Keyed on the record's own fields, never on `boards`: that array is rebuilt
   * by every 2 s port scan, so an array dependency would re-run `git rev-list`
   * twice a minute for as long as a card stayed open. These two strings only
   * change when the board is actually flashed again.
   */
  useEffect(() => {
    if (!selectedRec) {
      setDrift(null);
      return;
    }
    let cancelled = false;
    setDrift(null);
    projectDrift(selectedRec.project_dir, selectedRec.commit)
      .then((d) => {
        if (!cancelled) setDrift(d);
      })
      .catch(() => {
        // The command only fails when the directory cannot be reached at all.
        if (!cancelled) setDrift({ kind: "missing" });
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedRec?.project_dir, selectedRec?.commit]);

  /** Arming belongs to the card that was clicked, not to the next one. */
  useEffect(() => setArmed(null), [selected]);

  const openProject = (entry: FleetEntry) => {
    const rec = entry.last_flash;
    if (!rec) return;
    const cost = openCost();
    if (cost && armed !== entry.id) {
      setArmed(entry.id);
      return;
    }
    onOpenProject(rec.project_dir);
  };

  const saveNickname = async (id: string) => {
    setWorking(true);
    try {
      setBoards(await setBoardNickname(id, draft));
      setEditing(null);
      notify(draft.trim() ? `Renamed to “${draft.trim()}”` : "Nickname cleared");
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  const doForget = async (entry: FleetEntry) => {
    setConfirmForget(null);
    setWorking(true);
    try {
      setBoards(await forgetBoard(entry.id));
      notify(`Forgot ${fleetDisplayName(entry)}`);
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  /**
   * Ask esptool for the board's real MAC. This resets the board, so it is only
   * ever explicit — nothing here runs it automatically.
   */
  const doIdentify = async (port: string, previousId?: string) => {
    setWorking(true);
    onStreamStart();
    notify(`Identifying the board on ${port} via esptool…`);
    try {
      setBoards(await identifyBoard(port, previousId ?? null));
      notify("✓ Board identified by MAC");
      apply(await fleetSync(ports));
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  /**
   * What this board is carrying, and the way back to it.
   *
   * A real button rather than a click handler on the card: the expander has to
   * be reachable from the keyboard, and the card already holds four other
   * controls that a card-wide handler would have to fight with.
   *
   * Only the record is shown collapsed. The tag, branch, commit and drift are
   * behind the click because drift costs a `git rev-list`, and running one per
   * board on every 2 s port scan is exactly the churn `fleet_sync` avoids.
   */
  const flashBlock = (entry: FleetEntry) => {
    const rec = entry.last_flash;
    if (!rec) {
      return (
        <div className="fleet-meta fleet-flash-none">
          no flash recorded — flash it from Bancada and it will remember
        </div>
      );
    }
    const open = selected === entry.id;
    const isCurrent = rec.project_dir === openSketchDir;
    const missing = open && drift?.kind === "missing";
    const cost = openCost();
    const isArmed = armed === entry.id;

    return (
      <div className="fleet-flash">
        <button
          className="fleet-flash-summary"
          aria-expanded={open}
          onClick={() => setSelected(open ? null : entry.id)}
          title={rec.project_dir}
        >
          <span className="fleet-flash-caret" aria-hidden="true">
            {open ? "▾" : "▸"}
          </span>
          <span className="fleet-flash-project">{projectName(rec.project_dir)}</span>
          <span className="fleet-flash-tag">{rec.tag}</span>
          <span className="fleet-flash-when">{ago(rec.at, nowSecs)}</span>
        </button>

        {open && (
          <div className="fleet-flash-detail">
            <div className="fleet-meta fleet-flash-path">{rec.project_dir}</div>
            <div className="fleet-meta">
              {rec.branch ? `on ${rec.branch}` : "on a detached HEAD"} ·{" "}
              {rec.commit.slice(0, 8)} · flashed {when(rec.at)}
            </div>
            <div className="fleet-meta">
              {drift === null
                ? "checking the project…"
                : drift.kind === "missing"
                  ? "that folder is no longer there"
                  : driftLabel(drift.kind === "ahead" ? drift.commits : null, rec.tag)}
            </div>
            {isArmed && cost && (
              <div className="fleet-flash-warn">
                {cost} — click Open again to continue
              </div>
            )}
            <div className="fleet-actions">
              <div className="spacer" />
              {isCurrent ? (
                <span className="fleet-flash-current">open now</span>
              ) : (
                <button
                  className={`btn small ${isArmed ? "danger" : "primary"}`}
                  disabled={working || missing}
                  onClick={() => openProject(entry)}
                  title={
                    missing
                      ? "That folder is no longer there"
                      : `Open ${rec.project_dir}`
                  }
                >
                  {isArmed ? "Open anyway" : "Open project"}
                </button>
              )}
            </div>
          </div>
        )}
      </div>
    );
  };

  const card = (entry: FleetEntry) => {
    const live = isOnline(entry.id);
    const name = fleetDisplayName(entry);
    return (
      <div key={entry.id} className={live ? "fleet-card online" : "fleet-card"}>
        <div className="fleet-head">
          <span className="fleet-dot" title={live ? "Attached now" : "Not attached"} />
          {editing === entry.id ? (
            <input
              className="input"
              autoFocus
              value={draft}
              placeholder="nickname"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveNickname(entry.id);
                if (e.key === "Escape") setEditing(null);
              }}
              onBlur={() => saveNickname(entry.id)}
            />
          ) : (
            <button
              className="fleet-name"
              title="Click to rename"
              onClick={() => {
                setDraft(entry.nickname ?? "");
                setEditing(entry.id);
              }}
            >
              {name}
            </button>
          )}
          <span
            className={entry.id_kind === "mac" ? "fleet-badge mac" : "fleet-badge"}
            title={
              entry.id_kind === "mac"
                ? "Identified by hardware MAC address"
                : "Identified by USB serial number — not a hardware address"
            }
          >
            {entry.id_kind}
          </span>
        </div>

        <div className="fleet-id">{entry.id}</div>

        {(entry.chip_type || entry.board_name) && (
          <div className="fleet-meta">
            {[entry.chip_type, entry.board_name].filter(Boolean).join(" · ")}
          </div>
        )}

        {entry.fqbns.length > 0 && (
          <div className="fleet-meta" title="FQBNs this board has been built for">
            {entry.fqbns.join(", ")}
          </div>
        )}

        <div className="fleet-meta">
          {live ? (
            <>attached on {entry.last_port ?? "—"}</>
          ) : (
            <>
              last seen {ago(entry.last_seen, nowSecs)}
              {entry.last_port ? ` on ${entry.last_port}` : ""}
            </>
          )}
        </div>
        <div className="fleet-meta" title={`First seen ${when(entry.first_seen)}`}>
          known since {when(entry.first_seen)}
        </div>

        {flashBlock(entry)}

        <div className="fleet-actions">
          <div className="spacer" />
          {live && entry.id_kind === "serial" && entry.last_port && (
            <button
              className="btn small"
              disabled={working}
              onClick={() => doIdentify(entry.last_port!, entry.id)}
              title="Read the real MAC with esptool — this resets the board"
            >
              Identify
            </button>
          )}
          <button
            className={
              confirmForget === entry.id ? "btn small danger" : "btn small"
            }
            disabled={working}
            onClick={() =>
              confirmForget === entry.id
                ? doForget(entry)
                : setConfirmForget(entry.id)
            }
            title="Remove this board from the fleet"
          >
            {confirmForget === entry.id ? "Confirm?" : "Forget"}
          </button>
        </div>
      </div>
    );
  };

  // Attached boards first, then the rest by most-recently-seen.
  const sorted = [...boards].sort((a, b) => {
    const d = Number(isOnline(b.id)) - Number(isOnline(a.id));
    return d !== 0 ? d : b.last_seen - a.last_seen;
  });

  return (
    <div className="fleet-manager">
      <div className="panel-tabs">
        <span className="fleet-title">
          {boards.length} board{boards.length === 1 ? "" : "s"} ·{" "}
          {online.length} attached
        </span>
        <div className="spacer" />
        <button
          className="btn small"
          disabled={working}
          onClick={() =>
            fleetSync(ports)
              .then(apply)
              .catch((e) => notify(String(e), true))
          }
          title="Re-scan and record what is attached"
        >
          ⟳
        </button>
      </div>

      <div className="fleet-list">
        {sorted.map(card)}

        {unidentified.length > 0 && (
          <div className="fleet-unknown">
            <div className="fleet-meta">
              Attached but not identifiable — a USB-serial bridge reports its own
              serial, not the board's, so it cannot be told apart from another
              board behind the same bridge model.
            </div>
            {unidentified.map((dp) => (
              <div key={dp.port.address} className="fleet-card">
                <div className="fleet-head">
                  <span className="fleet-dot" />
                  <span className="fleet-name-static">
                    {portTitle(dp)}
                  </span>
                </div>
                <div className="fleet-id">{dp.port.address}</div>
                <div className="fleet-actions">
                  <div className="spacer" />
                  <button
                    className="btn small primary"
                    disabled={working}
                    onClick={() => doIdentify(dp.port.address)}
                    title="Read the MAC with esptool — ESP boards only; this resets the board"
                  >
                    Identify
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        {boards.length === 0 && unidentified.length === 0 && (
          <div className="empty-hint">
            No boards remembered yet. Plug one in and it is recorded
            automatically — an ESP32 with native USB reports its MAC address as
            its USB serial, so it is recognised without being touched.
          </div>
        )}
      </div>
    </div>
  );
}
