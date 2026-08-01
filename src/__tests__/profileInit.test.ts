import { describe, expect, it } from "vitest";
import { profileNameForFqbn } from "../profileInit";

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
