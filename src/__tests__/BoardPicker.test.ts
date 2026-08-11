import { describe, expect, it } from "vitest";
import { fallbackFqbnLabel } from "../components/BoardPicker";

describe("fallbackFqbnLabel", () => {
  it("extracts board name and options from a full FQBN", () => {
    expect(fallbackFqbnLabel("esp32:esp32:esp32c6:CDCOnBoot=cdc"))
      .toBe("esp32c6 (CDCOnBoot=cdc)");
  });

  it("handles multiple option key-value pairs", () => {
    expect(fallbackFqbnLabel("esp32:esp32:esp32s3:CDCOnBoot=cdc:PSRAM=enabled"))
      .toBe("esp32s3 (CDCOnBoot=cdc:PSRAM=enabled)");
  });

  it("shows only board name when no options present", () => {
    expect(fallbackFqbnLabel("arduino:avr:uno"))
      .toBe("uno");
  });

  it("returns the fqbn as-is for malformed input", () => {
    expect(fallbackFqbnLabel("invalid"))
      .toBe("invalid");
  });

  it("handles single-segment FQBNs gracefully", () => {
    expect(fallbackFqbnLabel("esp32"))
      .toBe("esp32");
  });
});
