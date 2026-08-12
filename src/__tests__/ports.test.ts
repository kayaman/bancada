import { describe, expect, it } from "vitest";
import {
  confidentBoardName,
  flashTargetMismatch,
  missingPortName,
  nextSelectedPort,
  portName,
  portOptions,
  portTitle,
  visibleBoard,
} from "../ports";
import type {
  DetectedPort,
  FleetEntry,
  FleetSnapshot,
  MatchingBoard,
} from "../api";

/** A fleet that knows one board, currently on `lastPort`. */
const fleetWith = (over: Partial<FleetEntry>, online = true): FleetSnapshot => {
  const entry: FleetEntry = {
    id: "3c:84:27:aa:bb:cc",
    id_kind: "mac",
    nickname: null,
    chip_type: null,
    board_name: null,
    fqbns: [],
    last_port: "/dev/ttyACM0",
    vid: null,
    pid: null,
    first_seen: 0,
    last_seen: 0,
    ...over,
  };
  return { boards: [entry], online: online ? [entry.id] : [], unidentified: [] };
};

const port = (address: string, protocol = "serial"): DetectedPort => ({
  port: {
    address,
    label: address,
    protocol,
    protocol_label: protocol === "serial" ? "Serial Port (USB)" : "Network Port",
    properties: {},
    hardware_id: "",
  },
  matching_boards: [],
});

describe("nextSelectedPort", () => {
  it("picks the first serial port when nothing is selected", () => {
    expect(nextSelectedPort([port("/dev/ttyACM0")], null)).toBe("/dev/ttyACM0");
  });

  it("keeps a still-attached choice rather than retargeting it", () => {
    // A rescan must never silently move an upload to a different board.
    const ports = [port("/dev/ttyACM0"), port("/dev/ttyUSB0")];
    expect(nextSelectedPort(ports, "/dev/ttyUSB0")).toBe("/dev/ttyUSB0");
  });

  // The regression this function exists for.
  it("drops a choice whose port has disappeared", () => {
    expect(nextSelectedPort([port("/dev/ttyACM1")], "/dev/ttyACM0")).toBe(
      "/dev/ttyACM1",
    );
  });

  it("clears the choice when nothing is attached at all", () => {
    expect(nextSelectedPort([], "/dev/ttyACM0")).toBeNull();
  });

  it("returns null for an empty scan with no prior choice", () => {
    expect(nextSelectedPort([], null)).toBeNull();
  });

  it("prefers a serial port over a network one when falling back", () => {
    // The UNO Q advertises itself over mDNS as well as USB; the serial port is
    // the one that can be flashed and monitored.
    const ports = [port("2804:7f0::1", "network"), port("/dev/ttyACM0")];
    expect(nextSelectedPort(ports, null)).toBe("/dev/ttyACM0");
  });

  it("honours a deliberately chosen network port that is still present", () => {
    const ports = [port("2804:7f0::1", "network"), port("/dev/ttyACM0")];
    expect(nextSelectedPort(ports, "2804:7f0::1")).toBe("2804:7f0::1");
  });

  it("falls back to a network port when it is all that is attached", () => {
    expect(nextSelectedPort([port("2804:7f0::1", "network")], null)).toBe(
      "2804:7f0::1",
    );
  });
});

const board = (fqbn: string, is_hidden = false): MatchingBoard => ({
  name: fqbn.split(":").pop() ?? fqbn,
  fqbn,
  is_hidden,
});

const withBoards = (address: string, boards: MatchingBoard[]): DetectedPort => ({
  ...port(address),
  matching_boards: boards,
});

describe("visibleBoard", () => {
  // arduino-cli lists the hidden esp32_family umbrella FIRST for native-USB
  // Espressif ports; [0] would compile against the umbrella FQBN.
  it("skips hidden umbrella entries", () => {
    const p = withBoards("/dev/ttyACM0", [
      board("esp32:esp32:esp32_family", true),
      board("esp32:esp32:esp32c6"),
    ]);
    expect(visibleBoard(p)?.fqbn).toBe("esp32:esp32:esp32c6");
  });

  it("returns null for a bridge port with no matching boards", () => {
    expect(visibleBoard(withBoards("/dev/ttyACM0", []))).toBeNull();
  });

  it("returns null when every match is hidden", () => {
    const p = withBoards("/dev/ttyACM0", [
      board("esp32:esp32:esp32_family", true),
    ]);
    expect(visibleBoard(p)).toBeNull();
  });
});

describe("confidentBoardName", () => {
  it("names a board matched on its own id", () => {
    const p = withBoards("/dev/ttyACM0", [board("arduino:avr:uno")]);
    expect(confidentBoardName(p)).toBe("uno");
  });

  it("refuses a family match, however tempting the sibling looks", () => {
    // The regression: arduino-cli offers the umbrella plus one arbitrary
    // member, so a plain ESP32-S3 dev board was being labelled "Ozobot
    // DRVKit". Mirrors core::fleet::board_name — being wrong is worse than
    // being silent, because this is what tells two boards apart.
    const p = withBoards("/dev/ttyACM0", [
      board("esp32:esp32:esp32_family", true),
      board("esp32:esp32:ozobot_drvkit"),
    ]);
    expect(confidentBoardName(p)).toBeNull();
    // visibleBoard still answers, because an FQBN to compile with is a
    // different question from a name to read.
    expect(visibleBoard(p)?.name).toBe("ozobot_drvkit");
  });

  it("refuses several named matches", () => {
    const p = withBoards("/dev/ttyACM0", [board("a:b:one"), board("a:b:two")]);
    expect(confidentBoardName(p)).toBeNull();
  });

  it("refuses a port with no matches at all", () => {
    expect(confidentBoardName(withBoards("/dev/ttyUSB0", []))).toBeNull();
  });
});

describe("portTitle", () => {
  it("prefers the nickname the user chose", () => {
    // The only one of these names the user picked, and the only thing that
    // tells two identical clones apart.
    const p = withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")]);
    expect(portTitle(p, fleetWith({ nickname: "porch-light" }))).toBe("porch-light");
  });

  it("falls back to a board arduino-cli is sure about", () => {
    const p = withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")]);
    expect(portTitle(p, fleetWith({}))).toBe("esp32c6");
  });

  it("does not print an arbitrary family sibling as the name", () => {
    const p = withBoards("/dev/ttyACM0", [
      board("esp32:esp32:esp32_family", true),
      board("esp32:esp32:ozobot_drvkit"),
    ]);
    expect(portTitle(p, fleetWith({ chip_type: "ESP32-S3" }))).toBe("ESP32-S3");
    expect(portTitle(p)).toBe("USB serial bridge");
  });

  it("falls back to the board the fleet remembers", () => {
    const p = withBoards("/dev/ttyACM0", []);
    expect(portTitle(p, fleetWith({ board_name: "Arduino Uno" }))).toBe("Arduino Uno");
  });

  it("names a port with no identity honestly", () => {
    expect(portTitle(withBoards("/dev/ttyACM0", []))).toBe("USB serial bridge");
    expect(portTitle(port("/dev/ttyACM0", "network"))).toBe("Network port");
  });

  it("ignores a nickname on a board that is not online", () => {
    // `last_port` is a memory, and the kernel reuses ttyACM0 for whatever is
    // plugged in next — the nickname must not migrate onto the newcomer.
    const p = withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")]);
    expect(portTitle(p, fleetWith({ nickname: "porch-light" }, false))).toBe("esp32c6");
  });

  it("ignores a blank nickname", () => {
    const p = withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")]);
    expect(portTitle(p, fleetWith({ nickname: "   " }))).toBe("esp32c6");
  });
});

describe("portName", () => {
  it("puts the name first and the device path second", () => {
    const p = withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")]);
    expect(portName(p, fleetWith({ nickname: "porch-light" }))).toBe(
      "porch-light · /dev/ttyACM0",
    );
  });

  it("still names a bridge port", () => {
    expect(portName(withBoards("/dev/ttyUSB0", []))).toBe(
      "USB serial bridge · /dev/ttyUSB0",
    );
  });
});

describe("missingPortName", () => {
  it("uses the fleet's memory when it has one", () => {
    // Matched without the online check — the whole point is that it is gone.
    expect(missingPortName("/dev/ttyACM0", fleetWith({ nickname: "esp32-spare" }, false))).toBe(
      "esp32-spare · not attached",
    );
  });

  it("falls back to the bare address", () => {
    expect(missingPortName("/dev/ttyUSB7")).toBe("/dev/ttyUSB7 · not attached");
  });
});

describe("portOptions", () => {
  it("labels a port with the standard name", () => {
    const opts = portOptions(
      [withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")])],
      null,
    );
    expect(opts).toEqual([
      { address: "/dev/ttyACM0", label: "esp32c6 · /dev/ttyACM0", missing: false },
    ]);
  });

  it("uses the nickname when the fleet knows one", () => {
    const opts = portOptions(
      [withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")])],
      null,
      fleetWith({ nickname: "porch-light" }),
    );
    expect(opts[0].label).toBe("porch-light · /dev/ttyACM0");
  });

  it("names a bridge port rather than showing a bare path", () => {
    const opts = portOptions([withBoards("/dev/ttyACM0", [])], null);
    expect(opts[0].label).toBe("USB serial bridge · /dev/ttyACM0");
  });

  it("appends a flagged entry for a selected port that is not attached", () => {
    // A sketch.yaml-pinned port that is absent used to leave the <select>
    // matching no <option>, rendering the control blank.
    const opts = portOptions([withBoards("/dev/ttyACM0", [])], "/dev/ttyUSB7");
    expect(opts).toContainEqual({
      address: "/dev/ttyUSB7",
      label: "/dev/ttyUSB7 · not attached",
      missing: true,
    });
  });

  it("adds nothing extra when the selection is attached", () => {
    const opts = portOptions([withBoards("/dev/ttyACM0", [])], "/dev/ttyACM0");
    expect(opts).toHaveLength(1);
  });
});

describe("flashTargetMismatch", () => {
  it("flags a profile aimed at a different board than the port reports", () => {
    // The 2026-08-09 bench incident: sketch.yaml still targeted the UNO Q
    // while a classic Uno sat on the selected port.
    expect(
      flashTargetMismatch("arduino:zephyr:unoq", "arduino:avr:uno"),
    ).toBe(true);
  });

  it("accepts a matching board even when the profile pins options", () => {
    expect(
      flashTargetMismatch(
        "esp32:esp32:esp32s3:CDCOnBoot=cdc,FlashSize=16M",
        "esp32:esp32:esp32s3",
      ),
    ).toBe(false);
  });

  it("stays quiet when the port reports no identity (bridge)", () => {
    expect(flashTargetMismatch("arduino:avr:uno", undefined)).toBe(false);
    expect(flashTargetMismatch("arduino:avr:uno", "")).toBe(false);
  });

  it("stays quiet when the profile has no fqbn", () => {
    expect(flashTargetMismatch(undefined, "arduino:avr:uno")).toBe(false);
  });
});
