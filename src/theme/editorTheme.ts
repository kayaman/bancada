// A CodeMirror theme built from Bancada's own tokens.
//
// Before this the editor was pinned to `@codemirror/theme-one-dark`, which is
// fine while the app is permanently dark and absurd the moment it is not: a
// light window with a black editor well in the middle of it. Generating the
// editor theme from the same `ThemeColors` the chrome uses means the two can
// never disagree, including for themes that do not exist yet.
//
// A note on what this is NOT. VS Code themes carry `tokenColors` — TextMate
// scope selectors, an open namespace where every grammar invents its own
// names. CodeMirror highlights off Lezer parse trees against a small CLOSED
// set of tags. There is no faithful automatic translation between the two, and
// the mapping below does not pretend to be one: it is a deliberate,
// hand-assigned palette for the tags that actually occur in Arduino C++. When
// `.vsix` import lands, its `tokenColors` will be reduced onto these same
// tags — approximating a 600-key scope namespace with a dozen roles — and that
// approximation is a design decision, not a bug to be fixed later.

import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";

import { withAlpha } from "./contrast";
import type { Theme } from "./tokens";

/** The editor chrome: everything that is not a syntax token. */
function editorChrome(theme: Theme): Extension {
  const c = theme.colors;
  const dark = theme.appearance === "dark";
  // Selection and the active line are washes over the editor ground rather
  // than opaque colours, so they stay correct whatever the ground is.
  const selection = withAlpha(c.accent, dark ? 0.25 : 0.2);
  const activeLine = withAlpha(c.text, dark ? 0.045 : 0.05);

  return EditorView.theme(
    {
      "&": {
        color: c.text,
        backgroundColor: c.bg,
      },
      ".cm-content": {
        caretColor: c.accent,
      },
      ".cm-cursor, .cm-dropCursor": {
        borderLeftColor: c.accent,
      },
      // CodeMirror splits selection painting between the focused and blurred
      // cases and uses `!important` in its own base theme, so both selectors
      // and the flag are required or the selection silently stays default.
      "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        {
          backgroundColor: `${selection} !important`,
        },
      ".cm-activeLine": {
        backgroundColor: activeLine,
      },
      ".cm-gutters": {
        backgroundColor: c.bg,
        color: c.textDim,
        border: "none",
        borderRight: `1px solid ${c.border}`,
      },
      ".cm-activeLineGutter": {
        backgroundColor: activeLine,
        color: c.text,
      },
      ".cm-foldPlaceholder": {
        backgroundColor: c.bgRaised,
        border: `1px solid ${c.border}`,
        color: c.textDim,
      },
      ".cm-tooltip": {
        backgroundColor: c.bgRaised,
        border: `1px solid ${c.borderStrong}`,
        color: c.text,
      },
      ".cm-tooltip .cm-tooltip-arrow:before": {
        borderTopColor: "transparent",
        borderBottomColor: "transparent",
      },
      ".cm-tooltip .cm-tooltip-arrow:after": {
        borderTopColor: c.bgRaised,
        borderBottomColor: c.bgRaised,
      },
      ".cm-panels": {
        backgroundColor: c.bgPanel,
        color: c.text,
      },
      ".cm-searchMatch": {
        backgroundColor: withAlpha(c.warn, 0.3),
        outline: `1px solid ${c.warn}`,
      },
      ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: withAlpha(c.warn, 0.5),
      },
      ".cm-selectionMatch": {
        backgroundColor: withAlpha(c.accent, 0.18),
      },
      ".cm-matchingBracket, .cm-nonmatchingBracket": {
        backgroundColor: withAlpha(c.accent, 0.2),
        outline: `1px solid ${c.accentDim}`,
      },
    },
    { dark },
  );
}

/** Syntax colours, assigned by role rather than by imitating another theme.
 *
 *  Deliberately narrow. Arduino sketches are C++ with a small vocabulary, and
 *  a palette that gives every Lezer tag its own hue reads as confetti. Five
 *  roles carry almost all the signal: comments recede, literals and strings
 *  are distinct, keywords are the accent, and types/preprocessor sit between.
 *  `--warn` doubles as the string colour because it is the one token
 *  guaranteed legible on every ground the theme defines. */
function editorSyntax(theme: Theme): Extension {
  const c = theme.colors;
  return syntaxHighlighting(
    HighlightStyle.define(
      [
        { tag: t.comment, color: c.textDim, fontStyle: "italic" },
        { tag: [t.keyword, t.moduleKeyword], color: c.accent },
        { tag: [t.controlKeyword, t.operatorKeyword], color: c.accent },
        { tag: [t.string, t.special(t.string)], color: c.warn },
        { tag: [t.number, t.bool, t.null], color: c.success },
        { tag: [t.typeName, t.className, t.namespace], color: c.accent },
        { tag: [t.definition(t.variableName), t.function(t.variableName)], color: c.text },
        { tag: t.propertyName, color: c.text },
        { tag: [t.meta, t.processingInstruction], color: c.error },
        { tag: t.operator, color: c.textDim },
        { tag: t.punctuation, color: c.textDim },
        { tag: t.invalid, color: c.error },
        { tag: [t.link, t.url], color: c.accent, textDecoration: "underline" },
        { tag: t.strong, fontWeight: "bold" },
        { tag: t.emphasis, fontStyle: "italic" },
      ],
      { themeType: theme.appearance },
    ),
  );
}

/** The extension to hand CodeMirror's `theme` prop. */
export function editorTheme(theme: Theme): Extension[] {
  return [editorChrome(theme), editorSyntax(theme)];
}
