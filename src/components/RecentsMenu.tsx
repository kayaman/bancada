import { useRef, useState } from "react";
import { loadSettings, type AppSettings } from "../api";
import Menu from "./Menu";

interface Props {
  onOpen: (dir: string) => void;
}

/** Dropdown of recently opened projects. Self-contained: reads the list fresh
 *  from settings on every open, so it never needs App-level plumbing. */
export default function RecentsMenu({ onOpen }: Props) {
  const btnRef = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const [recents, setRecents] = useState<string[]>([]);

  const toggle = async () => {
    if (anchor) {
      setAnchor(null);
      return;
    }
    // Outside a Tauri shell (plain vite in a browser) the call rejects and
    // the menu simply shows the empty state.
    const s = await loadSettings().catch(() => ({}) as AppSettings);
    setRecents(s.recent_projects ?? []);
    const r = btnRef.current?.getBoundingClientRect();
    if (r) setAnchor({ x: r.left, y: r.bottom + 4 });
  };

  const close = () => setAnchor(null);

  return (
    <>
      <button
        ref={btnRef}
        className="btn icon"
        onClick={toggle}
        title="Recent projects"
        aria-label="Recent projects"
      >
        ▾
      </button>
      {anchor && (
        <Menu x={anchor.x} y={anchor.y} onClose={close} anchorRef={btnRef}>
          {recents.length === 0 ? (
            <button className="ctx-item" disabled>
              No recent projects
            </button>
          ) : (
            recents.map((dir) => (
              <button
                key={dir}
                className="ctx-item"
                title={dir}
                onClick={() => {
                  close();
                  onOpen(dir);
                }}
              >
                {dir.split("/").pop()}
              </button>
            ))
          )}
        </Menu>
      )}
    </>
  );
}
