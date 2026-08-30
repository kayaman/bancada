// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import AppearanceMenu from "../AppearanceMenu";
import { BUILTIN_THEMES } from "../../theme/themes";
import { DENSITIES, DENSITY_LABEL } from "../../theme/density";
import type { ThemePrefs } from "../../theme/themePrefs";

afterEach(cleanup);

const PREFS: ThemePrefs = { themeId: "bancada-dark", density: "compact" };

function setup(prefs: ThemePrefs = PREFS) {
  const onChange = vi.fn();
  render(<AppearanceMenu prefs={prefs} onChange={onChange} />);
  return { onChange, user: userEvent.setup() };
}

const trigger = () => screen.getByRole("button", { name: "Appearance" });

describe("AppearanceMenu", () => {
  it("names its trigger for assistive tech and starts closed", () => {
    setup();
    expect(trigger().getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("group", { name: "Appearance" })).toBeNull();
  });

  it("opens on click and reports itself expanded", async () => {
    const { user } = setup();
    await user.click(trigger());
    expect(trigger().getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("group", { name: "Appearance" })).toBeTruthy();
  });

  it("offers every built-in theme and every density", async () => {
    const { user } = setup();
    await user.click(trigger());
    for (const t of BUILTIN_THEMES) {
      expect(screen.getByRole("radio", { name: new RegExp(t.name) })).toBeTruthy();
    }
    for (const d of DENSITIES) {
      expect(
        screen.getByRole("radio", { name: new RegExp(DENSITY_LABEL[d]) }),
      ).toBeTruthy();
    }
  });

  it("groups the two axes as separate radiogroups", async () => {
    // One flat menu would let a screen reader read "3 of 6" for a theme, and
    // the two choices are independent.
    const { user } = setup();
    await user.click(trigger());
    const themes = screen.getByRole("radiogroup", { name: "Theme" });
    const density = screen.getByRole("radiogroup", { name: "Density" });
    expect(within(themes).getAllByRole("radio")).toHaveLength(BUILTIN_THEMES.length);
    expect(within(density).getAllByRole("radio")).toHaveLength(DENSITIES.length);
  });

  it("marks the current selection checked in both groups", async () => {
    const { user } = setup({ themeId: "bancada-light", density: "comfortable" });
    await user.click(trigger());
    expect(screen.getByRole("radio", { name: /Bancada Light/ }).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("radio", { name: /Comfortable/ }).getAttribute("aria-checked")).toBe("true");
    expect(screen.getByRole("radio", { name: /Bancada Dark/ }).getAttribute("aria-checked")).toBe("false");
  });

  it("reports a theme choice without disturbing the density", async () => {
    const { onChange, user } = setup({ themeId: "bancada-dark", density: "normal" });
    await user.click(trigger());
    await user.click(screen.getByRole("radio", { name: /Bancada Light/ }));
    expect(onChange).toHaveBeenCalledWith({
      themeId: "bancada-light",
      density: "normal",
    });
  });

  it("reports a density choice without disturbing the theme", async () => {
    const { onChange, user } = setup({ themeId: "bancada-contrast", density: "compact" });
    await user.click(trigger());
    await user.click(screen.getByRole("radio", { name: /Comfortable/ }));
    expect(onChange).toHaveBeenCalledWith({
      themeId: "bancada-contrast",
      density: "comfortable",
    });
  });

  it("says out loud that a larger density can crowd the toolbar", async () => {
    // The bar has no overflow of its own, so this is a real consequence and
    // the user should meet it here rather than on the bench with Flash gone.
    const { user } = setup();
    await user.click(trigger());
    expect(screen.getByText(/crowd the toolbar/i)).toBeTruthy();
  });

  it("closes on Escape", async () => {
    const { user } = setup();
    await user.click(trigger());
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "Appearance" })).toBeNull();
  });
});
