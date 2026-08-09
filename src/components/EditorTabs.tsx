import { useCallback, useState } from "react";
import { tabLabel } from "../editorTabs";
import Menu from "./Menu";

interface Props {
  /** Open tabs (rel_paths) in open order. */
  tabs: string[];
  active: string | null;
  /** rel_paths with unsaved buffer edits — shown with the repo's existing dirty marker style. */
  dirty: ReadonlySet<string>;
  /** Tab awaiting "close again to discard" confirmation (unsaved close was requested once). */
  armed: string | null;
  onSelect: (rel: string) => void;
  onClose: (rel: string) => void;
  onCloseOthers: (rel: string) => void;
  onCloseAll: () => void;
}

/** One tab per open file, in open order — the sidebar's file tree may be
 *  hidden behind another panel, this strip never is. Unlike the old
 *  every-file-is-a-tab strip, tabs here track an explicit open set the
 *  caller owns; this component only renders and dispatches intent. */
export default function EditorTabs({
  tabs,
  active,
  dirty,
  armed,
  onSelect,
  onClose,
  onCloseOthers,
  onCloseAll,
}: Props) {
  const [menu, setMenu] = useState<{ rel: string; x: number; y: number } | null>(null);

  // Stable, as Menu's dismissal effect asks of its onClose.
  const closeMenu = useCallback(() => setMenu(null), []);

  return (
    <div className="panel-tabs editor-tabs">
      {tabs.map((rel) => {
        const isActive = rel === active;
        const isArmed = rel === armed;
        const classes = ["tab"];
        if (isActive) classes.push("active");
        if (isArmed) classes.push("danger");
        return (
          <div
            key={rel}
            className={classes.join(" ")}
            title={isArmed ? "unsaved — close again to discard" : rel}
            role="tab"
            aria-selected={isActive}
            tabIndex={0}
            onClick={() => onSelect(rel)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelect(rel);
              }
            }}
            onAuxClick={(e) => {
              if (e.button === 1) onClose(rel);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              setMenu({ rel, x: e.clientX, y: e.clientY });
            }}
          >
            <span className="tab-label">{tabLabel(rel)}</span>
            {dirty.has(rel) && <span className="tab-dot">●</span>}
            <button
              type="button"
              className="tab-close"
              title={`Close ${tabLabel(rel)}`}
              aria-label={`Close ${tabLabel(rel)}`}
              onClick={(e) => {
                e.stopPropagation();
                onClose(rel);
              }}
            >
              ✕
            </button>
          </div>
        );
      })}
      {tabs.length === 0 && <span className="editor-tabs-empty">no file open</span>}
      <div className="spacer" />
      {menu && (
        <Menu x={menu.x} y={menu.y} onClose={closeMenu}>
          <button
            className="ctx-item"
            role="menuitem"
            onClick={() => {
              closeMenu();
              onCloseOthers(menu.rel);
            }}
          >
            Close others
          </button>
          <button
            className="ctx-item"
            role="menuitem"
            onClick={() => {
              closeMenu();
              onCloseAll();
            }}
          >
            Close all
          </button>
        </Menu>
      )}
    </div>
  );
}
