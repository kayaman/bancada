import { describe, expect, it } from "vitest";
import {
  effectiveRetargetFqbn,
  initialFqbn,
  profileNameForFqbn,
  submitPlan,
} from "../profileInit";

// Mirrors core's profile_name_for_fqbn (core/src/project.rs) — same inputs,
// same outputs, so the suggested name matches what the backend would derive.
describe("profileNameForFqbn", () => {
  it("uses the board segment", () => {
    expect(profileNameForFqbn("esp32:esp32:esp32s3")).toBe("esp32s3");
  });

  it("sanitizes illegal characters to underscores", () => {
    expect(profileNameForFqbn("esp32:esp32:esp32 s3!")).toBe("esp32_s3");
  });

  it("falls back to the sanitized whole when there is no board segment", () => {
    expect(profileNameForFqbn("esp32")).toBe("esp32");
  });

  it("falls back to `default` when nothing usable remains", () => {
    expect(profileNameForFqbn(":::")).toBe("default");
  });
});

describe("submitPlan", () => {
  it("retarget targets the selected profile, not the name field", () => {
    expect(submitPlan("retarget", "esp32s3", "ignored-name", "esp32:esp32:esp32s3")).toEqual({
      kind: "retarget",
      profile: "esp32s3",
      fqbn: "esp32:esp32:esp32s3",
    });
  });

  it("add carries copyLibsFrom from the currently selected profile", () => {
    expect(submitPlan("add", "esp32s3", "uno", "arduino:avr:uno")).toEqual({
      kind: "create",
      profile: "uno",
      fqbn: "arduino:avr:uno",
      copyLibsFrom: "esp32s3",
    });
  });

  it("bootstrap carries no copyLibsFrom, even with a stray currentProfile", () => {
    expect(submitPlan("bootstrap", "esp32s3", "uno", "arduino:avr:uno")).toEqual({
      kind: "create",
      profile: "uno",
      fqbn: "arduino:avr:uno",
      copyLibsFrom: undefined,
    });
  });

  it("bootstrap with no currentProfile also carries no copyLibsFrom", () => {
    expect(submitPlan("bootstrap", null, "uno", "arduino:avr:uno")).toEqual({
      kind: "create",
      profile: "uno",
      fqbn: "arduino:avr:uno",
      copyLibsFrom: undefined,
    });
  });

  it("is null for retarget without a currentProfile", () => {
    expect(submitPlan("retarget", null, "", "arduino:avr:uno")).toBeNull();
  });

  it("is null for create modes with a blank (or whitespace-only) trimmed name", () => {
    expect(submitPlan("bootstrap", null, "   ", "arduino:avr:uno")).toBeNull();
    expect(submitPlan("add", "esp32s3", "", "arduino:avr:uno")).toBeNull();
  });

  it("is null for any mode with a blank fqbn", () => {
    expect(submitPlan("retarget", "esp32s3", "", "")).toBeNull();
    expect(submitPlan("bootstrap", null, "uno", "")).toBeNull();
    expect(submitPlan("add", "esp32s3", "uno", "   ")).toBeNull();
  });
});

describe("initialFqbn", () => {
  it("retarget preselects the profile's current board", () => {
    expect(initialFqbn("retarget", "esp32:esp32:esp32s3", "arduino:avr:uno")).toBe(
      "esp32:esp32:esp32s3",
    );
  });

  it("bootstrap preselects the board detected on the selected port", () => {
    expect(initialFqbn("bootstrap", null, "arduino:avr:uno")).toBe("arduino:avr:uno");
  });

  it("add also preselects the detected board, not the current profile's", () => {
    expect(initialFqbn("add", "esp32:esp32:esp32s3", "arduino:avr:uno")).toBe("arduino:avr:uno");
  });

  it("falls back to the empty string when nothing is available", () => {
    expect(initialFqbn("retarget", null, null)).toBe("");
    expect(initialFqbn("bootstrap", null, null)).toBe("");
  });
});

describe("effectiveRetargetFqbn", () => {
  it("keeps the current fqbn verbatim when the picked board has the same base", () => {
    expect(
      effectiveRetargetFqbn("esp32:esp32:esp32c6", "esp32:esp32:esp32c6:CDCOnBoot=cdc"),
    ).toBe("esp32:esp32:esp32c6:CDCOnBoot=cdc");
  });

  it("uses the picked fqbn when it's a genuinely different board", () => {
    expect(
      effectiveRetargetFqbn("arduino:avr:uno", "esp32:esp32:esp32c6:CDCOnBoot=cdc"),
    ).toBe("arduino:avr:uno");
  });

  it("uses the picked fqbn when there's no current fqbn", () => {
    expect(effectiveRetargetFqbn("esp32:esp32:esp32c6", null)).toBe("esp32:esp32:esp32c6");
    expect(effectiveRetargetFqbn("esp32:esp32:esp32c6", "")).toBe("esp32:esp32:esp32c6");
    expect(effectiveRetargetFqbn("esp32:esp32:esp32c6", "   ")).toBe("esp32:esp32:esp32c6");
  });

  it("returns the current fqbn (trivially unchanged) when neither carries options", () => {
    expect(effectiveRetargetFqbn("esp32:esp32:esp32c6", "esp32:esp32:esp32c6")).toBe(
      "esp32:esp32:esp32c6",
    );
  });

  it("handles surrounding whitespace on the picked value when comparing bases", () => {
    expect(
      effectiveRetargetFqbn("  esp32:esp32:esp32c6  ", "esp32:esp32:esp32c6:CDCOnBoot=cdc"),
    ).toBe("esp32:esp32:esp32c6:CDCOnBoot=cdc");
  });
});
