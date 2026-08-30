// The runtime token writer — the one primitive the whole UI layer rests on.
//
// A theme is a set of token values. A density is a set of token values. An
// imported VS Code theme will be a set of token values. All three arrive here
// and become CSS custom properties on the document root, which is the only
// place the stylesheet reads colour or size from. Build this once and
// "support light mode", "let me pick a density" and "load a .vsix" stop being
// three features and become three callers.
//
// Split in two on purpose: `tokenDeclarations` is pure and carries all the
// judgement, `applyTokens` is the four lines that touch the DOM. The tests
// exercise the first and barely need the second.

import { withAlpha } from "./contrast";
import { metricsFor, type Density } from "./density";
import {
  COLOR_VAR,
  DERIVED_VAR,
  FONT_VAR,
  SPACE_VAR,
  TINT_ALPHA,
  type Theme,
  type ColorToken,
  type FontToken,
  type SpaceToken,
} from "./tokens";

/** Every custom property a theme+density pair sets, as `--name` -> value.
 *
 *  Pure: no DOM, no globals. Feed it to a real root, a test double, or a
 *  string builder for a preview iframe. */
export function tokenDeclarations(
  theme: Theme,
  density: Density,
): Record<string, string> {
  const m = metricsFor(density);
  const out: Record<string, string> = {};

  for (const [token, cssVar] of Object.entries(COLOR_VAR)) {
    out[cssVar] = theme.colors[token as ColorToken];
  }
  for (const [token, cssVar] of Object.entries(FONT_VAR)) {
    out[cssVar] = `${m.font[token as FontToken]}px`;
  }
  for (const [token, cssVar] of Object.entries(SPACE_VAR)) {
    out[cssVar] = `${m.space[token as SpaceToken]}px`;
  }

  // Translucent washes, mixed from the theme's own semantic colours so they
  // can never disagree with the text they sit behind.
  out[DERIVED_VAR.errorTint] = withAlpha(theme.colors.error, TINT_ALPHA.errorTint);
  out[DERIVED_VAR.errorTintStrong] = withAlpha(
    theme.colors.error,
    TINT_ALPHA.errorTintStrong,
  );
  out[DERIVED_VAR.successTint] = withAlpha(
    theme.colors.success,
    TINT_ALPHA.successTint,
  );

  // The serial monitor virtualises its rows against a fixed row height that
  // exists in two places: this custom property and `ROW_HEIGHT` in
  // `serial/virtualize.ts`. They must agree or the monitor computes offsets
  // for rows of one height and paints rows of another — the list drifts
  // further out of register the further you scroll. `metricsFor` is the single
  // source; `SerialMonitor` reads it through `serialRowHeight()` below.
  out["--serial-row-h"] = `${m.serialRow}px`;

  return out;
}

/** Row height in px for the current density, for the virtualiser that cannot
 *  read CSS. The counterpart to `--serial-row-h` above; keep them together so
 *  the coupling stays visible. */
export function serialRowHeight(density: Density): number {
  return metricsFor(density).serialRow;
}

/** Writes the tokens onto a root element.
 *
 *  `color-scheme` is set from the theme's appearance because it is what makes
 *  native chrome — `<select>` popups, scrollbars, form controls — render dark
 *  or light. Without it a light theme keeps black scrollbars and the illusion
 *  breaks at the first dropdown.
 *
 *  `data-theme` / `data-density` are mirrored onto the element as attributes
 *  so a stylesheet rule can branch on them for the rare case a token cannot
 *  express (a shadow that must lighten on a light ground, say). */
export function applyTokens(
  theme: Theme,
  density: Density,
  root: HTMLElement = document.documentElement,
): void {
  const decls = tokenDeclarations(theme, density);
  for (const [name, value] of Object.entries(decls)) {
    root.style.setProperty(name, value);
  }
  root.style.setProperty("color-scheme", theme.appearance);
  root.setAttribute("data-theme", theme.id);
  root.setAttribute("data-appearance", theme.appearance);
  root.setAttribute("data-density", density);
}
