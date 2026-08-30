import { useCallback, useRef, useState } from "react";

import {
  DENSITIES,
  DENSITY_HINT,
  DENSITY_LABEL,
  type Density,
} from "../theme/density";
import { BUILTIN_THEMES } from "../theme/themes";
import type { ThemePrefs } from "../theme/themePrefs";
import Menu from "./Menu";

interface Props {
  prefs: ThemePrefs;
  onChange: (p: ThemePrefs) => void;
}

type Anchor = { x: number; y: number };

/**
 * Theme and density picker.
 *
 * Follows `ProjectMenu`/`GitPill`: `anchor` doubles as the open flag, `close`
 * is stable because `Menu`'s dismissal effect re-subscribes when `onClose`
 * changes, and `anchorRef` is passed so the trigger's own mousedown does not
 * read as "outside".
 *
 * `role="group"`, not the default `"menu"`: the popover holds two radio
 * groups, and a screen reader entering menu navigation would skip the
 * controls entirely (the reason `GitPill` does the same).
 *
 * It sits in the build cluster next to Usage rather than with the project
 * actions, for Usage's own reason — it is an application setting, not
 * something about the open sketch. The trigger is icon-only because that
 * cluster is pinned `flex: none` and every pixel it takes is a pixel the rest
 * of the bar cannot use.
 */
export default function AppearanceMenu({ prefs, onChange }: Props) {
  const btnRef = useRef<HTMLButtonElement>(null);
  const [anchor, setAnchor] = useState<Anchor | null>(null);

  const close = useCallback(() => setAnchor(null), []);

  const toggle = () => {
    if (anchor) return close();
    const r = btnRef.current?.getBoundingClientRect();
    if (!r) return;
    // Below the button and right-aligned to it: this control lives at the far
    // right of the bar, so a left-aligned popover would open off-window and
    // be clamped back with its edge under the cursor.
    setAnchor({ x: r.right - 232, y: r.bottom + 4 });
  };

  return (
    <>
      <button
        ref={btnRef}
        className="btn icon"
        onClick={toggle}
        title="Theme and density"
        aria-label="Appearance"
        aria-haspopup="dialog"
        aria-expanded={anchor !== null}
      >
        ◐
      </button>
      {anchor && (
        <Menu
          x={anchor.x}
          y={anchor.y}
          onClose={close}
          anchorRef={btnRef}
          role="group"
          ariaLabel="Appearance"
        >
          <div className="appearance-menu">
            <div className="appearance-section" role="radiogroup" aria-label="Theme">
              <div className="appearance-heading">Theme</div>
              {BUILTIN_THEMES.map((t) => (
                <button
                  key={t.id}
                  className="ctx-item appearance-option"
                  role="radio"
                  aria-checked={prefs.themeId === t.id}
                  onClick={() => onChange({ ...prefs, themeId: t.id })}
                >
                  <span
                    className="appearance-swatch"
                    aria-hidden="true"
                    style={{
                      background: t.colors.bgPanel,
                      borderColor: t.colors.borderStrong,
                    }}
                  >
                    <i style={{ background: t.colors.accent }} />
                    <i style={{ background: t.colors.text }} />
                    <i style={{ background: t.colors.warn }} />
                  </span>
                  <span className="appearance-label">{t.name}</span>
                  {prefs.themeId === t.id && (
                    <span className="appearance-tick" aria-hidden="true">
                      ✓
                    </span>
                  )}
                </button>
              ))}
            </div>

            <div className="appearance-section" role="radiogroup" aria-label="Density">
              <div className="appearance-heading">Density</div>
              {DENSITIES.map((d: Density) => (
                <button
                  key={d}
                  className="ctx-item appearance-option"
                  role="radio"
                  aria-checked={prefs.density === d}
                  title={DENSITY_HINT[d]}
                  onClick={() => onChange({ ...prefs, density: d })}
                >
                  <span className="appearance-label">{DENSITY_LABEL[d]}</span>
                  {prefs.density === d && (
                    <span className="appearance-tick" aria-hidden="true">
                      ✓
                    </span>
                  )}
                </button>
              ))}
              {/* Said plainly rather than discovered on the bench: the bar has
                  no overflow of its own, so a larger density can push Verify
                  and Flash past the right edge where nothing can reach them. */}
              <div className="appearance-note">
                Larger sizes can crowd the toolbar — Compact is the original.
              </div>
            </div>
          </div>
        </Menu>
      )}
    </>
  );
}
