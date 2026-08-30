import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";
import { applyTokens } from "./theme/apply";
import { loadThemePrefs } from "./theme/themePrefs";
import { themeById } from "./theme/themes";

// Paint the chosen theme BEFORE React mounts.
//
// styles.css carries the default palette in :root so the window is never
// unstyled, but that default is Bancada Dark. Mounting first and theming
// afterwards means anyone on the light theme watches a dark window flash past
// on every launch. Reading localStorage is synchronous, so doing it here costs
// nothing and removes the flash entirely — which is the whole reason the
// preference lives in localStorage rather than in settings.json behind IPC.
const prefs = loadThemePrefs(window.localStorage);
applyTokens(themeById(prefs.themeId), prefs.density);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
