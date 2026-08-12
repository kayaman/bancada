import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { loadSettings, type AppSettings } from "../api";
import { projectButtonLabel, projectMenuItems, type ProjectAction } from "../toolbarModel";
import Menu from "./Menu";

interface Props {
  sketchDir: string | null;
  onOpen: () => void;
  onOpenRecent: (dir: string) => void;
  onNew: () => void;
  onDuplicate: () => void;
  onRename: () => void;
}

type Anchor = { x: number; y: number };

/**
 * The toolbar's single project affordance: names the open project, and holds
 * every action on it.
 *
 * Replaces a row of five buttons. It follows `GitPill`'s shape — `anchor`
 * doubles as the open flag, `close` is stable because `Menu`'s dismissal
 * effect re-subscribes when `onClose` changes, and `anchorRef` is passed so
 * the trigger's own mousedown does not read as "outside".
 *
 * What is offered, and what is disabled and why, lives in `toolbarModel.ts` —
 * no component in this repo is reachable from a test.
 */
export default function ProjectMenu(props: Props) {
  const btnRef = useRef<HTMLButtonElement>(null);
  const recentRef = useRef<HTMLButtonElement>(null);
  const loadingRef = useRef(false);
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const [subAnchor, setSubAnchor] = useState<Anchor | null>(null);
  const [recents, setRecents] = useState<string[]>([]);

  const close = useCallback(() => {
    setSubAnchor(null);
    setAnchor(null);
  }, []);
  const closeSub = useCallback(() => setSubAnchor(null), []);

  // Escape closes the submenu first, then the menu. Both `Menu`s listen for
  // Escape on window in the bubble phase, so without this they would close
  // together. Capture runs before either, and stops the event there.
  useLayoutEffect(() => {
    if (!subAnchor) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setSubAnchor(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [subAnchor]);

  const toggle = () => {
    if (anchor) {
      close();
      return;
    }
    const r = btnRef.current?.getBoundingClientRect();
    if (r) setAnchor({ x: r.left, y: r.bottom + 4 });
  };

  // Recents load when the submenu opens, not the menu, so opening the menu is
  // never gated on an IPC round trip. The guard is `RecentsMenu`'s: this
  // handler awaits, and a double-click would otherwise pass the `subAnchor`
  // check twice and leave the submenu open instead of toggled shut.
  const toggleRecent = async () => {
    if (loadingRef.current) return;
    if (subAnchor) {
      setSubAnchor(null);
      return;
    }
    loadingRef.current = true;
    try {
      // Outside a Tauri shell the call rejects and the submenu shows its
      // empty state rather than nothing at all.
      const s = await loadSettings().catch(() => ({}) as AppSettings);
      setRecents(s.recent_projects ?? []);
      const r = recentRef.current?.getBoundingClientRect();
      if (r) setSubAnchor({ x: r.right + 2, y: r.top - 4 });
    } finally {
      loadingRef.current = false;
    }
  };

  const run = (id: ProjectAction) => {
    close();
    if (id === "open") props.onOpen();
    else if (id === "new") props.onNew();
    else if (id === "duplicate") props.onDuplicate();
    else props.onRename();
  };

  const [openItem, ...rest] = projectMenuItems({ sketchDir: props.sketchDir });

  const row = (item: (typeof rest)[number]) => (
    <button
      key={item.id}
      className={`ctx-item${item.accel ? " with-accel" : ""}`}
      role="menuitem"
      disabled={!!item.disabledReason}
      title={item.disabledReason}
      onClick={() => run(item.id)}
    >
      {item.label}
      {item.accel && <span className="ctx-accel">{item.accel}</span>}
    </button>
  );

  return (
    <>
      <button
        ref={btnRef}
        className="btn project-btn"
        onClick={toggle}
        title={props.sketchDir ?? "Open a project folder"}
        aria-haspopup="menu"
        aria-expanded={anchor !== null}
      >
        <span className="project-name">📁 {projectButtonLabel(props.sketchDir)}</span>
        <span aria-hidden="true">▾</span>
      </button>
      {anchor && (
        <Menu x={anchor.x} y={anchor.y} onClose={close} anchorRef={btnRef}>
          {row(openItem)}
          <button
            ref={recentRef}
            className="ctx-item with-accel"
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={subAnchor !== null}
            onClick={toggleRecent}
          >
            Recent
            <span className="ctx-accel" aria-hidden="true">
              ▸
            </span>
          </button>
          <div className="ctx-sep" />
          {rest.map(row)}
          {subAnchor && (
            <Menu
              x={subAnchor.x}
              y={subAnchor.y}
              onClose={closeSub}
              anchorRef={recentRef}
            >
              {recents.length === 0 ? (
                <button className="ctx-item" role="menuitem" disabled>
                  No recent projects
                </button>
              ) : (
                recents.map((dir) => (
                  <button
                    key={dir}
                    className="ctx-item"
                    role="menuitem"
                    title={dir}
                    onClick={() => {
                      close();
                      props.onOpenRecent(dir);
                    }}
                  >
                    {dir.split("/").filter(Boolean).pop()}
                  </button>
                ))
              )}
            </Menu>
          )}
        </Menu>
      )}
    </>
  );
}
