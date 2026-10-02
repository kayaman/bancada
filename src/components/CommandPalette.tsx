// CommandPalette — modal quick-switcher over a flat command list.
// Owns only its query and highlighted row; the command list comes from App.

import { useEffect, useMemo, useRef, useState } from "react";
import { type Command, filterCommands, moveIndex } from "../commandPalette";

interface Props {
  commands: Command[];
  onClose: () => void;
}

export default function CommandPalette({ commands, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const results = useMemo(() => filterCommands(commands, query), [commands, query]);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const runAt = (i: number) => {
    const c = results[i];
    if (!c || c.disabled) return;
    onClose();
    c.run();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setIndex((i) => moveIndex(i, 1, results.length));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setIndex((i) => moveIndex(i, -1, results.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      runAt(index);
    }
  };

  const active = Math.min(index, Math.max(results.length - 1, 0));

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <input
          ref={inputRef}
          className="palette-input"
          placeholder="Type a command…"
          aria-label="Search commands"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-list"
          aria-activedescendant={results.length ? `palette-opt-${active}` : undefined}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
        />
        <ul className="palette-list" id="palette-list" role="listbox">
          {results.length === 0 && <li className="palette-empty">No matching commands</li>}
          {results.map((c, i) => (
            <li
              key={c.id}
              id={`palette-opt-${i}`}
              role="option"
              aria-selected={i === active}
              aria-disabled={c.disabled || undefined}
              className={`palette-item${i === active ? " active" : ""}${c.disabled ? " disabled" : ""}`}
              onMouseEnter={() => setIndex(i)}
              onClick={() => runAt(i)}
            >
              <span className="palette-group">{c.group}</span>
              <span className="palette-label">{c.label}</span>
              {c.shortcut && <kbd className="palette-kbd">{c.shortcut}</kbd>}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
