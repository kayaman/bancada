import { describe, expect, it } from "vitest";
import {
  composeFqbn,
  defaultSelection,
  parseFqbn,
  silentSerialWarning,
} from "../boardOptions";
import type { ConfigOption } from "../boardOptions";

/** One option, values given as `value` or `value*` for the selected one. */
const opt = (option: string, ...values: string[]): ConfigOption => ({
  option,
  option_label: option,
  values: values.map((v) => ({
    value: v.replace(/\*$/, ""),
    value_label: v.replace(/\*$/, ""),
    selected: v.endsWith("*"),
  })),
});

/** The real ESP32-S3 shape, trimmed to three of its seventeen options. */
const S3_OPTIONS: ConfigOption[] = [
  {
    option: "CDCOnBoot",
    option_label: "USB CDC On Boot",
    values: [
      { value: "default", value_label: "Disabled", selected: true },
      { value: "cdc", value_label: "Enabled", selected: false },
    ],
  },
  opt("FlashSize", "4M*", "8M", "16M"),
  opt("PSRAM", "disabled*", "enabled", "opi"),
];

describe("defaultSelection", () => {
  it("reads the board's own defaults off the value list", () => {
    expect(defaultSelection(S3_OPTIONS)).toEqual({
      CDCOnBoot: "default",
      FlashSize: "4M",
      PSRAM: "disabled",
    });
  });

  it("returns an empty map for a board with no options", () => {
    expect(defaultSelection([])).toEqual({});
  });

  it("leaves out an option with no value flagged selected", () => {
    // Rather than claiming values[0] is the default: a wrong default makes
    // composeFqbn drop the user's matching choice from the FQBN silently.
    expect(defaultSelection([opt("UploadSpeed", "921600", "115200")])).toEqual({});
  });
});

describe("composeFqbn", () => {
  it("returns the bare base when nothing was changed", () => {
    // The whole point: 17 options spelled out is unreadable, and re-writes
    // sketch.yaml on every core update that shifts a default.
    expect(
      composeFqbn("esp32:esp32:esp32s3", S3_OPTIONS, defaultSelection(S3_OPTIONS)),
    ).toBe("esp32:esp32:esp32s3");
  });

  it("appends only the option the user actually changed", () => {
    expect(
      composeFqbn("esp32:esp32:esp32s3", S3_OPTIONS, {
        ...defaultSelection(S3_OPTIONS),
        CDCOnBoot: "cdc",
      }),
    ).toBe("esp32:esp32:esp32s3:CDCOnBoot=cdc");
  });

  it("emits several options in the order the board lists them", () => {
    // Stable across runs — not at the mercy of object key order.
    const selection = { PSRAM: "opi", FlashSize: "16M", CDCOnBoot: "cdc" };
    expect(composeFqbn("esp32:esp32:esp32s3", S3_OPTIONS, selection)).toBe(
      "esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PSRAM=opi",
    );
  });

  it("ignores a key the board does not offer", () => {
    // A menu left over from a core that has since been updated; inventing an
    // option for it only produces an FQBN the compiler rejects.
    expect(
      composeFqbn("esp32:esp32:esp32s3", S3_OPTIONS, { Wombat: "yes" }),
    ).toBe("esp32:esp32:esp32s3");
  });

  it("keeps a value the board does not offer, for an option it does", () => {
    // The user's pinned choice: arduino-cli refusing it by name beats Bancada
    // quietly flashing something else.
    expect(
      composeFqbn("esp32:esp32:esp32s3", S3_OPTIONS, { FlashSize: "32M" }),
    ).toBe("esp32:esp32:esp32s3:FlashSize=32M");
  });

  it("replaces the options on a base that already carries some", () => {
    expect(
      composeFqbn("esp32:esp32:esp32s3:FlashSize=16M", S3_OPTIONS, {
        CDCOnBoot: "cdc",
      }),
    ).toBe("esp32:esp32:esp32s3:CDCOnBoot=cdc");
  });

  it("leaves a board with no options alone", () => {
    expect(composeFqbn("arduino:avr:uno", [], {})).toBe("arduino:avr:uno");
  });
});

describe("parseFqbn", () => {
  it("splits at the fourth colon, not the first", () => {
    // The base has colons of its own; a naive split makes `esp32` an option.
    expect(parseFqbn("esp32:esp32:esp32s3:CDCOnBoot=cdc")).toEqual({
      base: "esp32:esp32:esp32s3",
      selection: { CDCOnBoot: "cdc" },
    });
  });

  it("reads a comma-separated list", () => {
    expect(parseFqbn("esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M")).toEqual({
      base: "esp32:esp32:esp32s3",
      selection: { CDCOnBoot: "cdc", FlashSize: "16M" },
    });
  });

  it("gives a bare FQBN an empty selection", () => {
    expect(parseFqbn("arduino:avr:uno")).toEqual({
      base: "arduino:avr:uno",
      selection: {},
    });
  });

  it("hands back a malformed string whole", () => {
    // Guessing loses the only thing the user wrote down.
    expect(parseFqbn("nonsense")).toEqual({ base: "nonsense", selection: {} });
    expect(parseFqbn("")).toEqual({ base: "", selection: {} });
    expect(parseFqbn("esp32:esp32")).toEqual({
      base: "esp32:esp32",
      selection: {},
    });
    expect(parseFqbn("esp32:esp32:esp32s3:garbage")).toEqual({
      base: "esp32:esp32:esp32s3:garbage",
      selection: {},
    });
  });

  it("ignores an empty trailing item", () => {
    expect(parseFqbn("esp32:esp32:esp32s3:CDCOnBoot=cdc,")).toEqual({
      base: "esp32:esp32:esp32s3",
      selection: { CDCOnBoot: "cdc" },
    });
  });

  it("round-trips a non-default selection through composeFqbn", () => {
    const fqbn = "esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M,PSRAM=opi";
    const { base, selection } = parseFqbn(fqbn);
    expect(composeFqbn(base, S3_OPTIONS, selection)).toBe(fqbn);
  });

  it("round-trips a bare FQBN", () => {
    const { base, selection } = parseFqbn("esp32:esp32:esp32s3");
    expect(composeFqbn(base, S3_OPTIONS, selection)).toBe("esp32:esp32:esp32s3");
  });

  it("drops an option written out at its default value", () => {
    // Not byte-for-byte, deliberately: `CDCOnBoot=default` and the bare form
    // name the same board, and the short one is the one worth storing.
    const { base, selection } = parseFqbn("esp32:esp32:esp32s3:CDCOnBoot=default");
    expect(composeFqbn(base, S3_OPTIONS, selection)).toBe("esp32:esp32:esp32s3");
  });
});

describe("silentSerialWarning", () => {
  // The 2026 bench incident: compiles clean, flashes clean, monitor never
  // prints a byte, and the hours go into re-checking the wiring.
  it("warns for an ESP32-C6 on ttyACM with CDCOnBoot unset", () => {
    const w = silentSerialWarning("/dev/ttyACM0", "esp32:esp32:esp32c6");
    expect(w).toBe(
      "Serial output will not reach /dev/ttyACM0: with CDCOnBoot left at its " +
        "default this board sends Serial to the UART0 peripheral rather than " +
        "the USB port, so the monitor stays empty while the upload itself " +
        "looks fine. Set USB CDC On Boot to Enabled in the profile's board " +
        "options.",
    );
  });

  it("does not offer the other USB port as a way out", () => {
    // An S3 DevKit has two USB-C sockets, and on the one this was found with
    // the UART socket prints nothing at all — UART0 goes to the peripheral,
    // not necessarily to anything openable. Enabling CDC is the fix that
    // holds regardless of how the board is wired, so the message must not
    // send anyone hunting for a second port.
    const w = silentSerialWarning("/dev/ttyACM0", "esp32:esp32:esp32s3") ?? "";
    expect(w).not.toContain("UART pins");
    expect(w).not.toMatch(/other port|UART port/i);
    expect(w).toContain("USB CDC On Boot to Enabled");
  });

  it("warns when CDCOnBoot is written out as its default", () => {
    expect(
      silentSerialWarning("/dev/ttyACM0", "esp32:esp32:esp32s3:CDCOnBoot=default"),
    ).toContain("CDCOnBoot");
  });

  it("warns for every native-USB variant", () => {
    for (const id of ["esp32s2", "esp32s3", "esp32c3", "esp32c6", "esp32h2"]) {
      expect(silentSerialWarning("/dev/ttyACM0", `esp32:esp32:${id}`)).not.toBeNull();
    }
  });

  it("names the port it is talking about", () => {
    expect(silentSerialWarning("/dev/ttyACM3", "esp32:esp32:esp32s3")).toContain(
      "/dev/ttyACM3",
    );
  });

  it("warns on a macOS usbmodem port", () => {
    expect(
      silentSerialWarning("/dev/cu.usbmodem14201", "esp32:esp32:esp32s3"),
    ).not.toBeNull();
    expect(
      silentSerialWarning("/dev/tty.usbmodem14201", "esp32:esp32:esp32s3"),
    ).not.toBeNull();
  });

  it("stays quiet once CDC is turned on", () => {
    expect(
      silentSerialWarning("/dev/ttyACM0", "esp32:esp32:esp32s3:CDCOnBoot=cdc"),
    ).toBeNull();
    expect(
      silentSerialWarning(
        "/dev/ttyACM0",
        "esp32:esp32:esp32s3:FlashSize=16M,CDCOnBoot=cdc",
      ),
    ).toBeNull();
  });

  it("stays quiet for a classic ESP32, which has no native USB", () => {
    expect(silentSerialWarning("/dev/ttyACM0", "esp32:esp32:esp32")).toBeNull();
  });

  it("stays quiet for a bridge port", () => {
    // A CH340 or CP210x is wired to the UART pins, where Serial arrives
    // whatever CDCOnBoot says.
    expect(silentSerialWarning("/dev/ttyUSB0", "esp32:esp32:esp32s3")).toBeNull();
    expect(
      silentSerialWarning("/dev/cu.usbserial-0001", "esp32:esp32:esp32s3"),
    ).toBeNull();
  });

  it("stays quiet on Windows, where the port name says nothing", () => {
    expect(silentSerialWarning("COM3", "esp32:esp32:esp32s3")).toBeNull();
  });

  it("stays quiet with nothing to judge", () => {
    expect(silentSerialWarning(null, "esp32:esp32:esp32s3")).toBeNull();
    expect(silentSerialWarning("/dev/ttyACM0", undefined)).toBeNull();
    expect(silentSerialWarning("/dev/ttyACM0", "")).toBeNull();
  });

  it("stays quiet for a non-Espressif board on a native-USB port", () => {
    // The UNO Q and the RP2040 both enumerate as ttyACM.
    expect(silentSerialWarning("/dev/ttyACM0", "arduino:zephyr:unoq")).toBeNull();
    expect(
      silentSerialWarning("/dev/ttyACM0", "rp2040:rp2040:rpipico"),
    ).toBeNull();
  });

  it("stays quiet for a malformed FQBN", () => {
    expect(silentSerialWarning("/dev/ttyACM0", "esp32s3")).toBeNull();
    expect(silentSerialWarning("/dev/ttyACM0", "esp32:esp32")).toBeNull();
  });

  it("stays quiet for a network port", () => {
    expect(silentSerialWarning("2804:7f0::1", "esp32:esp32:esp32s3")).toBeNull();
  });
});
