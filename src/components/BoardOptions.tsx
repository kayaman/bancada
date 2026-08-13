import { useCallback, useEffect, useRef, useState } from "react";
import Menu from "./Menu";
import { boardDetails, type ConfigOption } from "../api";
import { composeFqbn, defaultSelection, parseFqbn } from "../boardOptions";

interface Props {
  /** The profile's FQBN, which may already carry options. */
  fqbn: string;
  /** Called with the recomposed FQBN whenever a value changes. */
  onChange: (fqbn: string) => void;
  disabled?: boolean;
}

/**
 * The board's own configuration menus, as a popover off the board picker.
 *
 * A profile could only ever pin a bare FQBN, so options the Arduino IDE
 * exposes as menus were unreachable — including `CDCOnBoot`, which on an
 * ESP32-S3 decides whether `Serial` goes to the native USB port or to the
 * UART pins. A board that flashes perfectly and prints nothing is the
 * failure that motivated this.
 *
 * A popover rather than inline: `ProfileInit` is a deliberate one-row form
 * and the S3 alone exposes seventeen options. The common path — pick a
 * board, accept its defaults — is unchanged and needs no clicks here.
 */
export default function BoardOptions({ fqbn, onChange, disabled }: Props) {
  const btnRef = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null);
  const [options, setOptions] = useState<ConfigOption[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const { base, selection } = parseFqbn(fqbn);
  const close = useCallback(() => setAnchor(null), []);

  // Refetched per board: the option set is the board's, not the profile's.
  // Cleared first so the popover never shows the previous board's menus.
  useEffect(() => {
    setOptions(null);
    setError(null);
    if (!base) return;
    let cancelled = false;
    boardDetails(base)
      .then((d) => {
        if (!cancelled) setOptions(d.config_options ?? []);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [base]);

  const toggle = () => {
    if (anchor) {
      close();
      return;
    }
    const r = btnRef.current?.getBoundingClientRect();
    if (r) setAnchor({ x: r.left, y: r.bottom + 4 });
  };

  // What the board would build with today: its defaults, overridden by
  // whatever the profile spells out.
  const effective = options
    ? { ...defaultSelection(options), ...selection }
    : selection;

  const pick = (option: string, value: string) => {
    if (!options) return;
    onChange(composeFqbn(base, options, { ...effective, [option]: value }));
  };

  const changed = Object.keys(selection).length;
  const label = changed > 0 ? `Options · ${changed}` : "Options";
  const reason = !base
    ? "pick a board first"
    : error
      ? error
      : options && options.length === 0
        ? "this board has no configurable options"
        : null;

  return (
    <>
      <button
        ref={btnRef}
        className="btn"
        onClick={toggle}
        disabled={disabled || reason !== null}
        title={reason ?? "Board options pinned into this profile's FQBN"}
        aria-haspopup="menu"
        aria-expanded={anchor !== null}
      >
        {label}
      </button>
      {anchor && options && (
        <Menu
          x={anchor.x}
          y={anchor.y}
          onClose={close}
          anchorRef={btnRef}
          // Selects, not menu items — see Menu's `role`.
          role="group"
          ariaLabel="Board options"
        >
          <div className="board-options">
            {options.map((o) => (
              <label key={o.option} className="board-option">
                <span className="board-option-label" title={o.option}>
                  {o.option_label}
                </span>
                <select
                  className="select small"
                  value={effective[o.option] ?? ""}
                  onChange={(e) => pick(o.option, e.target.value)}
                >
                  {o.values.map((v) => (
                    <option key={v.value} value={v.value}>
                      {v.value_label || v.value}
                      {v.selected ? " (default)" : ""}
                    </option>
                  ))}
                </select>
              </label>
            ))}
            {/* What actually gets pinned — only what differs from the
                board's own defaults, so sketch.yaml stays readable. */}
            <div className="board-options-fqbn" title="Pinned in sketch.yaml">
              {composeFqbn(base, options, effective)}
            </div>
          </div>
        </Menu>
      )}
    </>
  );
}
