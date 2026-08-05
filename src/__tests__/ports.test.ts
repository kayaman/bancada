import { describe, expect, it } from "vitest";
import { nextSelectedPort, portOptions, visibleBoard } from "../ports";
import type { DetectedPort, MatchingBoard } from "../api";

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

describe("portOptions", () => {
  it("labels a port with its visible board name", () => {
    const opts = portOptions(
      [withBoards("/dev/ttyACM0", [board("esp32:esp32:esp32c6")])],
      null,
    );
    expect(opts).toEqual([
      { address: "/dev/ttyACM0", label: "/dev/ttyACM0 (esp32c6)", missing: false },
    ]);
  });

  it("labels a bridge port with its bare address", () => {
    const opts = portOptions([withBoards("/dev/ttyACM0", [])], null);
    expect(opts[0].label).toBe("/dev/ttyACM0");
  });

  it("appends a flagged entry for a selected port that is not attached", () => {
    // A sketch.yaml-pinned port that is absent used to leave the <select>
    // matching no <option>, rendering the control blank.
    const opts = portOptions([withBoards("/dev/ttyACM0", [])], "/dev/ttyUSB7");
    expect(opts).toContainEqual({
      address: "/dev/ttyUSB7",
      label: "/dev/ttyUSB7 (not attached)",
      missing: true,
    });
  });

  it("adds nothing extra when the selection is attached", () => {
    const opts = portOptions([withBoards("/dev/ttyACM0", [])], "/dev/ttyACM0");
    expect(opts).toHaveLength(1);
  });
});
