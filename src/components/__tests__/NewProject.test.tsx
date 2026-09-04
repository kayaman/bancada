// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import NewProject from "../NewProject";

const createProject = vi.fn();
const createIdfProject = vi.fn();
const setLastProjectPlatform = vi.fn().mockResolvedValue(undefined);

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

vi.mock("../../api", async () => {
  const actual = await vi.importActual<typeof import("../../api")>("../../api");
  const S3 = {
    id: "esp32-s3-devkitc-1",
    name: "ESP32-S3-DevKitC-1",
    target: "esp32s3",
    led: { gpio: 38, kind: "ws2812" },
    headers: [],
    sources: [],
    notes: [],
    usb: [],
    vendor: "Espressif",
    revision: "v1.1",
    module: "ESP32-S3-WROOM-1",
    boot_button: 0,
  };
  return {
    ...actual,
    loadSettings: vi.fn().mockResolvedValue({ last_new_project_parent: "/home/p" }),
    defaultProjectParent: vi.fn().mockResolvedValue("/home/p"),
    listAllBoards: vi.fn().mockResolvedValue([]),
    listSketchTemplates: vi
      .fn()
      .mockResolvedValue([{ id: "blink", label: "Blink", description: "hello world" }]),
    listIdfTemplates: vi
      .fn()
      .mockResolvedValue([
        { id: "hello", label: "Hello", description: "chip info" },
        { id: "blink", label: "Blink", description: "toggle a gpio" },
      ]),
    knownIdfTargets: vi.fn().mockResolvedValue([
      { id: "esp32", name: "ESP32", native_usb: false },
      { id: "esp32s3", name: "ESP32-S3", native_usb: true },
      { id: "esp32c6", name: "ESP32-C6", native_usb: true },
    ]),
    setLastProjectPlatform: (...a: unknown[]) => setLastProjectPlatform(...a),
    boardCatalog: vi.fn().mockResolvedValue({ boards: [S3], caveats: [] }),
    boardCandidates: vi.fn().mockResolvedValue([]),
    setLastProjectParent: vi.fn().mockResolvedValue(undefined),
    createProject: (...a: unknown[]) => createProject(...a),
    createIdfProject: (...a: unknown[]) => createIdfProject(...a),
  };
});

function setup() {
  const onCreated = vi.fn();
  render(
    <NewProject
      detectedFqbn={null}
      onCreated={onCreated}
      onCancel={vi.fn()}
      notify={vi.fn()}
    />,
  );
  return { onCreated };
}

const pickIdf = () => fireEvent.click(screen.getByRole("radio", { name: /ESP-IDF/ }));

beforeEach(() => {
  createProject.mockReset();
  setLastProjectPlatform.mockClear();
  createIdfProject.mockReset().mockResolvedValue({
    dir: "/home/p/node",
    name: "node",
    target: "esp32c6",
    board: null,
    files: [],
    under_git: true,
    git_error: null,
  });
});
afterEach(cleanup);

describe("NewProject platform choice", () => {
  it("offers both platforms and starts on Arduino", async () => {
    setup();
    const arduino = await screen.findByRole("radio", { name: /Arduino/ });
    const idf = screen.getByRole("radio", { name: /ESP-IDF/ });
    expect(arduino.getAttribute("aria-checked")).toBe("true");
    expect(idf.getAttribute("aria-checked")).toBe("false");

    fireEvent.click(idf);
    expect(idf.getAttribute("aria-checked")).toBe("true");
    expect(arduino.getAttribute("aria-checked")).toBe("false");
  });

  it("drops profile and libraries on the ESP-IDF side", async () => {
    // They are arduino-cli concepts. Rendering them disabled would claim the
    // form can do something it cannot.
    setup();
    await screen.findByText("Profile name");
    pickIdf();
    await waitFor(() => expect(screen.queryByText("Profile name")).toBeNull());
    expect(
      screen.queryByPlaceholderText("search the registry to add libraries…"),
    ).toBeNull();
  });

  it("asks for a target chip instead of a board", async () => {
    setup();
    pickIdf();
    expect(await screen.findByText("Target chip")).toBeTruthy();
    expect(screen.queryByText("Board")).toBeNull();
  });

  it("says an install is not needed to create the project", async () => {
    // The whole reason scaffolding is hermetic — worth stating where the
    // user is deciding, not only in the module docs.
    setup();
    pickIdf();
    expect(
      await screen.findByText(/No\s*ESP-IDF install is needed to create/),
    ).toBeTruthy();
  });

  it("creates through the ESP-IDF command, not the Arduino one", async () => {
    const { onCreated } = setup();
    pickIdf();
    await screen.findByText("Target chip");
    fireEvent.change(screen.getByPlaceholderText("blink_node"), {
      target: { value: "node" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));

    await waitFor(() => expect(createIdfProject).toHaveBeenCalled());
    expect(createProject).not.toHaveBeenCalled();
    expect(onCreated).toHaveBeenCalledWith("/home/p/node");
  });

  it("can create without any installed Arduino platform", async () => {
    // listAllBoards returns [] here. On the Arduino side that correctly
    // blocks Create; on the ESP-IDF side it must not, or the path that exists
    // to escape arduino-cli's world would be gated on arduino-cli's world.
    setup();
    pickIdf();
    await screen.findByText("Target chip");
    fireEvent.change(screen.getByPlaceholderText("blink_node"), {
      target: { value: "node" },
    });
    expect(
      (screen.getByRole("button", { name: "Create project" }) as HTMLButtonElement)
        .disabled,
    ).toBe(false);
  });
  it("names chips the way Espressif does, not the way tools spell them", () => {
    // "ESP32-S3" is how the silkscreen, the datasheet and every forum post
    // write it; `esp32s3` is a spelling only the toolchain uses.
    setup();
    pickIdf();
    return waitFor(() => {
      expect(screen.getByRole("option", { name: "ESP32-S3" })).toBeTruthy();
      expect(screen.queryByRole("option", { name: "esp32s3" })).toBeNull();
    });
  });

  it("still sends the tool spelling, not the pretty one", async () => {
    setup();
    pickIdf();
    await screen.findByText("Target chip");
    fireEvent.change(screen.getByTitle(/CONFIG_IDF_TARGET/), {
      target: { value: "esp32c6" },
    });
    fireEvent.change(screen.getByPlaceholderText("blink_node"), {
      target: { value: "node" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() =>
      expect(createIdfProject).toHaveBeenCalledWith(
        expect.anything(),
        "node",
        expect.anything(),
        "esp32c6",
        null,
      ),
    );
  });

  it("remembers the platform so a run of ESP-IDF projects need not re-pick it", async () => {
    setup();
    pickIdf();
    await screen.findByText("Target chip");
    fireEvent.change(screen.getByPlaceholderText("blink_node"), {
      target: { value: "node" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create project" }));
    await waitFor(() => expect(setLastProjectPlatform).toHaveBeenCalledWith("idf"));
  });
});
