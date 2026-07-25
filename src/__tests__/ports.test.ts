import { describe, expect, it } from "vitest";
import { nextSelectedPort } from "../ports";
import type { DetectedPort } from "../api";

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
