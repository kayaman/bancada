import { describe, expect, it } from "vitest";

import type { CircuitCatalog } from "../api";
import {
  addComponent,
  addConnection,
  fqbnTarget,
  newCircuitManifest,
  selectCircuitBoard,
} from "../circuitModel";

const catalog: CircuitCatalog = {
  version: 1,
  boards: [
    {
      id: "esp32-s3-devkitc-1",
      label: "S3",
      fqbn: "esp32:esp32:esp32s3",
      logic_voltage_v: 3.3,
      max_3v3_ma: 500,
      source_url: "https://example.test/s3",
      pins: [],
    },
    {
      id: "esp32-c6-devkitc-1",
      label: "C6",
      fqbn: "esp32:esp32:esp32c6",
      logic_voltage_v: 3.3,
      max_3v3_ma: 500,
      source_url: "https://example.test/c6",
      pins: [],
    },
  ],
  component_kinds: [
    { kind: "led", label: "LED", pins: ["anode", "cathode"], safety_hint: "resistor" },
  ],
};

describe("circuit editor model", () => {
  it("selects the catalog board by FQBN target while ignoring options", () => {
    const manifest = newCircuitManifest(
      catalog,
      "Bench",
      "esp32:esp32:esp32c6:CDCOnBoot=cdc",
    );
    expect(manifest.board.catalog_id).toBe("esp32-c6-devkitc-1");
    expect(fqbnTarget(manifest.board.fqbn)).toBe("esp32:esp32:esp32c6");
  });

  it("adds uniquely identified components and a guided board connection", () => {
    let manifest = newCircuitManifest(catalog, "Bench");
    manifest = addComponent(manifest, catalog.component_kinds[0]);
    manifest = addComponent(manifest, catalog.component_kinds[0]);
    manifest = addConnection(manifest);
    expect(manifest.components.map((item) => item.id)).toEqual(["led_1", "led_2"]);
    expect(manifest.connections[0]).toMatchObject({
      id: "wire_1",
      from: { target: "board", pin: "GND" },
      to: { target: "led_1", pin: "anode" },
    });
  });

  it("changes both catalog id and FQBN when the board changes", () => {
    const manifest = selectCircuitBoard(
      newCircuitManifest(catalog, "Bench"),
      catalog,
      "esp32-c6-devkitc-1",
    );
    expect(manifest.board).toEqual({
      catalog_id: "esp32-c6-devkitc-1",
      fqbn: "esp32:esp32:esp32c6",
    });
  });
});
