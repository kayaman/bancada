import { useState } from "react";
import type { BoardOption } from "../api";
import { groupByPlatform, visibleBoards } from "../boardSearch";

interface Props {
  boards: BoardOption[];
  /** Selected FQBN, or "" for none. */
  value: string;
  onChange: (fqbn: string) => void;
  title: string;
}

/** A grouped board <select> with a filter input in front — `board listall`
 *  returns hundreds of entries, so scrolling alone doesn't cut it. */
export default function BoardPicker({ boards, value, onChange, title }: Props) {
  const [query, setQuery] = useState("");
  const groups = groupByPlatform(visibleBoards(boards, query, value));

  return (
    <span className="board-picker">
      <input
        className="input board-search"
        placeholder="filter boards…"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        title="Filter by name, FQBN or platform"
      />
      <select
        className="select"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        title={title}
      >
        <option value="">— choose a board —</option>
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
  );
}
