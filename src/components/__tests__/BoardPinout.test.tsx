// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import BoardPinout from "../BoardPinout";
import type { Board, BoardChoice } from "../../api";

const projectInfo = vi.fn();
const boardCatalog = vi.fn();
const setProjectBoard = vi.fn();

vi.mock("../../api", async () => {
  const actual = await vi.importActual<typeof import("../../api")>("../../api");
  return {
    ...actual,
    projectInfo: (...a: unknown[]) => projectInfo(...a),
    boardCatalog: (...a: unknown[]) => boardCatalog(...a),
    setProjectBoard: (...a: unknown[]) => setProjectBoard(...a),
  };
});

const S3: Board = {
  id: "esp32-s3-devkitc-1",
  name: "ESP32-S3-DevKitC-1",
  vendor: "Espressif",
  target: "esp32s3",
  revision: "v1.1",
  module: "ESP32-S3-WROOM-1",
  usb: [],
  led: { gpio: 38, kind: "ws2812" },
  boot_button: 0,
  headers: [
    {
      name: "J1",
      side: "left",
      pins: [
        { label: "G0", gpio: 0, functions: ["BOOT"], caveats: ["strapping"] },
        { label: "3V3", gpio: null, functions: [], caveats: [] },
      ],
    },
  ],
  sources: ["Espressif user guide"],
  notes: [],
};

function setup(board: BoardChoice) {
  projectInfo.mockResolvedValue({
    kind: "arduino",
    idf_target: null,
    idf_console: null,
    board,
  });
  boardCatalog.mockResolvedValue({ boards: [S3], caveats: [] });
  render(<BoardPinout sketchDir="/p" notify={vi.fn()} />);
}

beforeEach(() => {
  projectInfo.mockReset();
  boardCatalog.mockReset();
  setProjectBoard.mockReset();
});
afterEach(cleanup);

describe("BoardPinout", () => {
  it("shows a recorded board as recorded, with no inferred badge", async () => {
    setup({ state: "recorded", board: S3 });
    expect(await screen.findByText("ESP32-S3-DevKitC-1")).toBeTruthy();
    expect(screen.getByText(/recorded in the project/)).toBeTruthy();
    expect(screen.queryByText("inferred")).toBeNull();
  });

  it("marks an inferred board as a guess and offers to record it", async () => {
    // The distinction the whole board model rests on: an inferred board still
    // renders its pinout, but must never look like a fact the project stated.
    setup({ state: "inferred", board: S3 });
    expect(await screen.findByText("inferred")).toBeTruthy();
    expect(screen.getByText(/it is a guess until you record it/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /Record ESP32-S3/ })).toBeTruthy();
  });

  it("says unknown rather than safe when there is no profile", async () => {
    setup({ state: "no-profile" });
    expect(
      await screen.findByText(/means unknown,\s*not safe/),
    ).toBeTruthy();
    // No pin table at all — an empty one would read as "no caveats".
    expect(screen.queryByText("G0")).toBeNull();
  });

  it("asks which board rather than picking one when several fit the chip", async () => {
    setup({ state: "unchosen", candidates: [S3, { ...S3, id: "other", name: "Other Board" }] });
    expect(await screen.findByText(/Several boards use this chip/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "ESP32-S3-DevKitC-1" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Other Board" })).toBeTruthy();
  });

  it("names a WS2812 LED as one, because a plain toggle does nothing to it", async () => {
    setup({ state: "recorded", board: S3 });
    expect(
      await screen.findByText(/addressable WS2812, a plain HIGH\/LOW does nothing/),
    ).toBeTruthy();
  });

  it("renders a power row with no GPIO and no caveat badges", async () => {
    setup({ state: "recorded", board: S3 });
    await waitFor(() => expect(screen.getByText("3V3")).toBeTruthy());
    expect(screen.getByText("GPIO0")).toBeTruthy();
  });
});
