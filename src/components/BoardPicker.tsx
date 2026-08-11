import { useState } from "react";
import type { BoardOption } from "../api";
import { filterBoards, groupByPlatform, listboxRows } from "../boardSearch";

interface Props {
  boards: BoardOption[];
  /** Selected FQBN, or "" for none. */
  value: string;
  onChange: (fqbn: string) => void;
  title: string;
}

/** Derive a fallback label for an FQBN with options (e.g. "esp32c6 (CDCOnBoot=cdc")
 *  when it doesn't match any listed board. Extracts board name (3rd colon segment)
 *  and options (remainder after 3rd colon). */
export function fallbackFqbnLabel(fqbn: string): string {
  const parts = fqbn.split(":");
  if (parts.length < 3) return fqbn;
  const boardName = parts[2];
  const optionsPart = parts.slice(3).join(":");
  return optionsPart ? `${boardName} (${optionsPart})` : boardName;
}

/** A grouped board <select> with a filter input in front. While the query
 *  is non-empty the select opens into a results listbox overlaying the
 *  content below (see .board-select-anchor); picking collapses it. */
export default function BoardPicker({ boards, value, onChange, title }: Props) {
  const [query, setQuery] = useState("");
  const open = query.trim().length > 0;
  const matches = open ? filterBoards(boards, query) : boards;
  const groups = groupByPlatform(matches);

  const pick = (fqbn: string) => {
    onChange(fqbn);
    setQuery(""); // collapse back to the closed select showing the pick
  };

  return (
    <span className="board-picker">
      <input
        className="input board-search"
        placeholder="filter boards…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && open && matches.length > 0) {
            e.preventDefault();
            pick(matches[0].fqbn);
          } else if (e.key === "Escape") {
            setQuery("");
          }
        }}
        title="Filter by name, FQBN or platform — results open below"
      />
      <span className="board-select-anchor">
        <select
          className={open ? "select board-select-open" : "select"}
          size={open ? listboxRows(matches.length, groups.length) : undefined}
          value={value}
          onChange={(e) => pick(e.target.value)}
          title={title}
        >
          {open ? (
            <>
              {/* The control's value must always have an option, even when
                  the current selection doesn't match the filter. */}
              {value && !matches.some((b) => b.fqbn === value) && (
                <option value={value} hidden />
              )}
              {matches.length === 0 && (
                <option value="" disabled>
                  — no boards match —
                </option>
              )}
            </>
          ) : (
            <>
              {value && !boards.some((b) => b.fqbn === value) ? (
                <option value={value}>{fallbackFqbnLabel(value)}</option>
              ) : (
                <option value="">— choose a board —</option>
              )}
            </>
          )}
          {groups.map(([platform, list]) => (
            <optgroup key={platform} label={platform}>
              {list.map((b) => (
                <option key={b.fqbn} value={b.fqbn}>
                  {b.name}
                </option>
              ))}
            </optgroup>
          ))}
        </select>
      </span>
    </span>
  );
}
