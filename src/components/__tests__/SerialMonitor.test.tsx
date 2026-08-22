// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

const saveMock = vi.fn(async (..._a: unknown[]) => "/tmp/cap.txt" as string | null);
const saveTextFileMock = vi.fn(async (..._a: unknown[]) => undefined);

vi.mock("@tauri-apps/plugin-dialog", () => ({
  save: (...a: unknown[]) => saveMock(...a),
}));
vi.mock("../../api", () => ({
  saveTextFile: (...a: unknown[]) => saveTextFileMock(...a),
}));

import SerialMonitor from "../SerialMonitor";
import { SerialStore, exportText } from "../../serial/serialStore";
import { UI_KEY, type StorageLike } from "../../serialPrefs";

function fakeStorage(seed: Record<string, string> = {}): StorageLike & {
  map: Map<string, string>;
} {
  const map = new Map(Object.entries(seed));
  return {
    map,
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
  };
}

const ts = 1_700_000_000_000;

type Props = Parameters<typeof SerialMonitor>[0];

function setup(over: Partial<Props> = {}) {
  const store = over.store ?? new SerialStore();
  const props: Props = {
    active: true,
    store,
    monitorOn: true,
    busy: false,
    portLabel: "/dev/ttyACM0",
    baud: 115200,
    baudSource: "default",
    sketchBaud: null,
    onBaudChange: vi.fn(),
    onUseSketchBaud: vi.fn(),
    connection: { state: "on" },
    onToggleMonitor: vi.fn(),
    onSend: vi.fn(async () => {}),
    notify: vi.fn(),
    storage: fakeStorage(),
    ...over,
  };
  const view = render(<SerialMonitor {...props} />);
  return { ...view, props, store };
}

/** One poll tick, plus the rAF the scroll handler may have queued. */
async function tick(ms = 150) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
  });
}

/** Let a click's promise chain settle without moving the clock. */
async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

const rows = () => document.querySelectorAll(".serial-row");

beforeEach(() => {
  vi.useFakeTimers();
  saveMock.mockClear();
  saveTextFileMock.mockClear();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("SerialMonitor toolbar", () => {
  it("names every control for assistive tech", () => {
    setup();
    expect(screen.getByLabelText("Baud rate")).toBeTruthy();
    expect(screen.getByLabelText("Line ending")).toBeTruthy();
    expect(screen.getByLabelText("Filter lines")).toBeTruthy();
    expect(screen.getByLabelText("Send to board")).toBeTruthy();
  });

  it("hides itself rather than unmounting when inactive", () => {
    setup({ active: false });
    const section = document.querySelector(".serial-monitor") as HTMLElement;
    expect(section.style.display).toBe("none");
    // Still mounted: the store keeps filling behind the hidden panel.
    expect(screen.getByLabelText("Baud rate")).toBeTruthy();
  });

  it("blocks Start during a flash and says why", () => {
    setup({ monitorOn: false, busy: true });
    const start = screen.getByRole("button", { name: "Start" }) as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    expect(start.title).toBe(
      "Flashing — the monitor restarts when the flash finishes",
    );
  });

  it("blocks Start with no port and says why", () => {
    setup({ monitorOn: false, portLabel: null });
    const start = screen.getByRole("button", { name: "Start" }) as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    expect(start.title).toBe("Select a port first");
    expect(screen.getByText("no port")).toBeTruthy();
  });

  it("shows the connection state as a status chip", () => {
    setup({ connection: { state: "retrying", attempt: 2, max: 5 } });
    const chip = document.querySelector(".serial-status") as HTMLElement;
    expect(chip.dataset.state).toBe("retrying");
    expect(chip.textContent).toContain("retrying 2/5");
  });

  it("reports a baud change", () => {
    const { props } = setup();
    fireEvent.change(screen.getByLabelText("Baud rate"), {
      target: { value: "74880" },
    });
    expect(props.onBaudChange).toHaveBeenCalledWith(74880);
  });

  it("offers a baud the list does not carry", () => {
    setup({ baud: 38400, baudSource: "sketch" });
    const select = screen.getByLabelText("Baud rate") as HTMLSelectElement;
    expect([...select.options].map((o) => o.value)).toContain("38400");
    expect(select.value).toBe("38400");
  });

  it("offers the sketch's rate back once the user has overridden it", () => {
    const { props } = setup({ baud: 9600, baudSource: "override", sketchBaud: 74880 });
    const btn = screen.getByRole("button", { name: "Use sketch's 74880" });
    expect(btn.title).toBe("Serial.begin(74880) found in the sketch");
    btn.click();
    expect(props.onUseSketchBaud).toHaveBeenCalled();
  });

  it("hides that button when the baud is not an override", () => {
    setup({ baud: 74880, baudSource: "sketch", sketchBaud: 74880 });
    expect(screen.queryByRole("button", { name: /Use sketch/ })).toBeNull();
  });
});

describe("SerialMonitor log", () => {
  it("shows lines the store gained since the last poll", async () => {
    const { store } = setup();
    expect(document.querySelector(".empty-hint")?.textContent).toBe(
      "no serial output yet — press Start",
    );
    act(() => void store.push("stdout", "hello board", ts));
    await tick();
    expect(screen.getByText("hello board")).toBeTruthy();
    expect(rows()).toHaveLength(1);
  });

  it("marks stderr and info rows", async () => {
    const { store } = setup();
    act(() => {
      store.push("stderr", "oops", ts);
      store.push("info", "monitor started", ts + 1);
    });
    await tick();
    expect(document.querySelector(".serial-row.stderr")?.textContent).toContain("oops");
    expect(document.querySelector(".serial-row.info")).toBeTruthy();
  });

  it("counts lines, and counts matches while filtering", async () => {
    const { store } = setup();
    act(() => {
      store.push("stdout", "temp=21", ts);
      store.push("stdout", "hum=40", ts + 1);
      store.push("stdout", "wind=3", ts + 2);
    });
    await tick();
    expect(document.querySelector(".serial-count")?.textContent).toBe("3 lines");

    fireEvent.change(screen.getByLabelText("Filter lines"), {
      target: { value: "temp" },
    });
    await tick();
    expect(document.querySelector(".serial-count")?.textContent).toBe("1 of 3 lines");
    expect(rows()).toHaveLength(1);
  });

  it("says so when the filter matches nothing", async () => {
    const { store } = setup();
    act(() => void store.push("stdout", "temp=21", ts));
    await tick();
    fireEvent.change(screen.getByLabelText("Filter lines"), {
      target: { value: "zzz" },
    });
    await tick();
    expect(document.querySelector(".empty-hint")?.textContent).toBe(
      "no lines match the filter",
    );
  });

  it("freezes on Pause and counts what arrived behind it", async () => {
    const { store } = setup();
    act(() => void store.push("stdout", "first", ts));
    await tick();
    screen.getByRole("button", { name: "⏸ Pause" }).click();
    act(() => {
      store.push("stdout", "second", ts + 1);
      store.push("stdout", "third", ts + 2);
    });
    await tick();
    expect(rows()).toHaveLength(1);
    expect(screen.getByRole("button", { name: "▶ Resume (2 buffered)" })).toBeTruthy();

    screen.getByRole("button", { name: "▶ Resume (2 buffered)" }).click();
    await tick();
    expect(rows()).toHaveLength(3);
  });

  it("clears on demand", async () => {
    const { store } = setup();
    act(() => void store.push("stdout", "gone", ts));
    await tick();
    screen.getByRole("button", { name: "Clear" }).click();
    await tick();
    expect(rows()).toHaveLength(0);
  });

  it("renders a window, not the whole ring", async () => {
    // 1000 rows in a 180px viewport: the DOM must hold the visible ten plus
    // overscan, not a thousand nodes.
    const spy = vi
      .spyOn(HTMLElement.prototype, "clientHeight", "get")
      .mockReturnValue(180);
    try {
      const store = new SerialStore();
      for (let i = 0; i < 1000; i++) store.push("stdout", `line ${i}`, ts + i);
      setup({ store });
      await tick();
      expect(rows().length).toBeGreaterThan(0);
      expect(rows().length).toBeLessThan(40);
      const spacer = document.querySelector(".serial-log-spacer") as HTMLElement;
      expect(spacer.style.height).toBe(`${1000 * 18}px`);
    } finally {
      spy.mockRestore();
    }
  });
});

describe("SerialMonitor send box", () => {
  it("sends on Enter with the chosen line ending and echoes a tx row", async () => {
    const { props, store } = setup();
    fireEvent.change(screen.getByLabelText("Line ending"), {
      target: { value: "nlcr" },
    });
    const box = screen.getByLabelText("Send to board") as HTMLInputElement;
    fireEvent.change(box, { target: { value: "AT" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await flush();
    expect(props.onSend).toHaveBeenCalledWith("AT\r\n");
    await tick();
    expect(document.querySelector(".serial-row.tx")?.textContent).toContain("AT");
    expect(box.value).toBe("");
    expect(store.snapshot().rows.at(-1)?.stream).toBe("tx");
  });

  it("recalls the last sent line with ArrowUp", async () => {
    setup();
    const box = screen.getByLabelText("Send to board") as HTMLInputElement;
    fireEvent.change(box, { target: { value: "AT+GMR" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await flush();
    expect(box.value).toBe("");
    fireEvent.keyDown(box, { key: "ArrowUp" });
    expect(box.value).toBe("AT+GMR");
    fireEvent.keyDown(box, { key: "Escape" });
    expect(box.value).toBe("");
  });

  it("reports a send failure through notify and keeps the text", async () => {
    const { props } = setup({
      onSend: vi.fn(async () => {
        throw new Error("port gone");
      }),
    });
    const box = screen.getByLabelText("Send to board") as HTMLInputElement;
    fireEvent.change(box, { target: { value: "AT" } });
    fireEvent.keyDown(box, { key: "Enter" });
    await flush();
    expect(props.notify).toHaveBeenCalledWith(expect.stringContaining("port gone"), true);
    expect(box.value).toBe("AT");
  });

  it("refuses to send while the monitor is off", () => {
    setup({ monitorOn: false });
    const send = screen.getByRole("button", { name: "Send" }) as HTMLButtonElement;
    expect(send.disabled).toBe(true);
    expect(send.title).toBe("Start the monitor first");
  });
});

describe("SerialMonitor prefs and export", () => {
  it("persists toolbar prefs to the injected storage", async () => {
    const storage = fakeStorage();
    setup({ storage });
    screen.getByRole("button", { name: "Timestamps" }).click();
    await tick();
    expect(JSON.parse(storage.map.get(UI_KEY)!).timestamps).toBe(true);

    fireEvent.change(screen.getByLabelText("Line ending"), { target: { value: "cr" } });
    await tick();
    expect(JSON.parse(storage.map.get(UI_KEY)!).lineEnding).toBe("cr");
  });

  it("reads prefs back on mount", () => {
    const storage = fakeStorage({
      [UI_KEY]: JSON.stringify({
        lineEnding: "cr",
        timestamps: true,
        autoscroll: false,
      }),
    });
    setup({ storage });
    expect((screen.getByLabelText("Line ending") as HTMLSelectElement).value).toBe("cr");
    expect(
      screen.getByRole("button", { name: "Timestamps" }).classList.contains("toggled"),
    ).toBe(true);
  });

  it("exports exactly the visible lines", async () => {
    const { store } = setup();
    act(() => {
      store.push("stdout", "temp=21", ts);
      store.push("stdout", "hum=40", ts + 1);
    });
    await tick();
    fireEvent.change(screen.getByLabelText("Filter lines"), {
      target: { value: "temp" },
    });
    await tick();
    screen.getByRole("button", { name: "Export" }).click();
    await flush();
    const visible = store.snapshot("temp").rows;
    expect(saveTextFileMock).toHaveBeenCalledWith(
      "/tmp/cap.txt",
      exportText(visible, { timestamps: false }),
    );
  });

  it("writes nothing when the save dialog is dismissed", async () => {
    const { store } = setup();
    act(() => void store.push("stdout", "temp=21", ts));
    await tick();
    saveMock.mockResolvedValueOnce(null);
    screen.getByRole("button", { name: "Export" }).click();
    await flush();
    expect(saveTextFileMock).not.toHaveBeenCalled();
  });
});
