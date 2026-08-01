import { describe, expect, it } from "vitest";
import { arrivals } from "../portWatch";
import type { FleetEntry, FleetSnapshot } from "../api";

const entry = (id: string, over: Partial<FleetEntry> = {}): FleetEntry => ({
  id,
  id_kind: "mac",
  nickname: null,
  chip_type: null,
  board_name: null,
  fqbns: [],
  last_port: null,
  vid: null,
  pid: null,
  first_seen: 0,
  last_seen: 0,
  ...over,
});

const snap = (boards: FleetEntry[], online: string[]): FleetSnapshot => ({
  boards,
  online,
  unidentified: [],
});

describe("arrivals", () => {
  it("announces nothing on the first sync after launch", () => {
    const s = snap([entry("aa:bb")], ["aa:bb"]);
    expect(arrivals(null, s)).toEqual([]);
  });

  it("reports a board that came online since the previous sync", () => {
    const s = snap(
      [entry("aa:bb", { nickname: "sonar-node", last_port: "/dev/ttyACM0" })],
      ["aa:bb"],
    );
    expect(arrivals([], s)).toEqual([
      { id: "aa:bb", name: "sonar-node", port: "/dev/ttyACM0" },
    ]);
  });

  it("ignores boards that were already online", () => {
    const s = snap([entry("aa:bb"), entry("cc:dd")], ["aa:bb", "cc:dd"]);
    expect(arrivals(["aa:bb", "cc:dd"], s)).toEqual([]);
  });

  it("falls back nickname -> board_name -> id for the display name", () => {
    const s = snap(
      [entry("aa:bb", { board_name: "ESP32S3 Dev Module" }), entry("cc:dd")],
      ["aa:bb", "cc:dd"],
    );
    expect(arrivals([], s).map((a) => a.name)).toEqual([
      "ESP32S3 Dev Module",
      "cc:dd",
    ]);
  });

  it("handles simultaneous arrivals", () => {
    const s = snap([entry("aa:bb"), entry("cc:dd")], ["aa:bb", "cc:dd"]);
    expect(arrivals([], s)).toHaveLength(2);
  });

  it("does not treat departures as arrivals", () => {
    const s = snap([entry("aa:bb")], []);
    expect(arrivals(["aa:bb"], s)).toEqual([]);
  });
});
