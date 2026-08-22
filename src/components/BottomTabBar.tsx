import { Fragment } from "react";
import { tabRow, type BottomTab } from "../bottomTabs";

interface Props {
  active: BottomTab;
  /** Tabs with content that arrived while hidden — dotted, except the active one. */
  unseen: Partial<Record<BottomTab, boolean>>;
  /** Optional per-tab counts (e.g. build errors); zero and absent both render nothing. */
  badges?: Partial<Record<BottomTab, number>>;
  onOpen: (t: BottomTab) => void;
  maximized: boolean;
  onToggleMaximize: () => void;
}

/** The bottom panel's whole header: one flat row of seven tabs, the former
 *  group boundaries left as thin separators, then the maximize toggle. What
 *  to render is decided by `tabRow` in `../bottomTabs` — add a tab there. */
export default function BottomTabBar({
  active,
  unseen,
  badges,
  onOpen,
  maximized,
  onToggleMaximize,
}: Props) {
  return (
    <div className="panel-tabs bottom-tabs">
      {tabRow(active, unseen, badges).map((item) => (
        <Fragment key={item.tab}>
          <button
            className={item.active ? "tab active" : "tab"}
            aria-current={item.active ? "true" : undefined}
            onClick={() => onOpen(item.tab)}
          >
            {item.label}
            {item.badge !== null && (
              <span className="tab-badge">{item.badge}</span>
            )}
            {item.dot && <span className="tab-dot">●</span>}
          </button>
          {item.separatorAfter && (
            <span
              className="tab-sep"
              role="separator"
              aria-orientation="vertical"
            />
          )}
        </Fragment>
      ))}
      <div className="spacer" />
      <button
        className="btn small icon"
        title={maximized ? "Restore panel" : "Maximize panel"}
        aria-label={maximized ? "Restore panel" : "Maximize panel"}
        onClick={onToggleMaximize}
      >
        {maximized ? "❐" : "⛶"}
      </button>
    </div>
  );
}
