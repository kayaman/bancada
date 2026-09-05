// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import SetupPanel, { idfState, toolState } from "../SetupPanel";
import type { SetupReport, ToolStatus } from "../../api";

const setupProbe = vi.fn();
const setupInstallArduinoCli = vi.fn();
const idfProbe = vi.fn();

vi.mock("../../api", async () => {
  const actual = await vi.importActual<typeof import("../../api")>("../../api");
  return {
    ...actual,
    setupProbe: (...a: unknown[]) => setupProbe(...a),
    setupInstallArduinoCli: (...a: unknown[]) => setupInstallArduinoCli(...a),
    idfProbe: (...a: unknown[]) => idfProbe(...a),
  };
});

const tool = (over: Partial<ToolStatus>): ToolStatus => ({
  id: "git",
  name: "git",
  purpose: "version control",
  required: false,
  ok: true,
  version: "git version 2.51.0",
  install: "sudo dnf install git",
  docs: "https://git-scm.com",
  installable: false,
  ...over,
});

const report = (over: Partial<SetupReport> = {}): SetupReport => ({
  tools: [
    tool({
      id: "arduino-cli",
      name: "arduino-cli",
      required: true,
      ok: false,
      version: undefined,
      install: "curl … | sh",
      installable: true,
    }),
    tool({}),
  ],
  serial: {
    access: { state: "missing", group: "dialout" },
    device: "/dev/ttyACM0",
    fix: "sudo usermod -aG dialout $USER",
  },
  path: "/usr/bin:/home/u/.local/bin",
  ...over,
});

describe("SetupPanel", () => {
  beforeEach(() => {
    setupProbe.mockReset().mockResolvedValue(report());
    setupInstallArduinoCli.mockReset();
    idfProbe.mockReset().mockResolvedValue({ ok: false, error: "ESP-IDF is not installed" });
  });
  afterEach(cleanup);

  it("lists each engine with its state, and an install command only for the missing ones", async () => {
    render(<SetupPanel onClose={() => {}} notify={() => {}} />);
    const cli = await screen.findByTestId("setup-tool-arduino-cli");
    expect(cli.textContent).toContain("not found — needed for Arduino projects");
    expect(cli.textContent).toContain("curl … | sh");
    expect(cli.querySelector("button.primary")?.textContent).toBe("Install");

    const git = screen.getByTestId("setup-tool-git");
    expect(git.textContent).toContain("git version 2.51.0");
    expect(git.querySelector("code")).toBeNull();
  });

  it("names the serial group to join, taken from the attached device", async () => {
    render(<SetupPanel onClose={() => {}} notify={() => {}} />);
    const serial = await screen.findByTestId("setup-serial");
    expect(serial.textContent).toContain("not in the dialout group");
    expect(serial.textContent).toContain("/dev/ttyACM0");
    expect(serial.textContent).toContain("sudo usermod -aG dialout $USER");
  });

  it("shows ESP-IDF from its own probe, missing or found", async () => {
    render(<SetupPanel onClose={() => {}} notify={() => {}} />);
    const idf = await screen.findByTestId("setup-tool-esp-idf");
    await waitFor(() => expect(idf.textContent).toContain("ESP-IDF is not installed"));
    expect(idf.querySelector("code")?.textContent).toContain("install.sh");
    expect(
      idfState({ ok: true, version: "v6.1", idf_path: "/home/u/esp/esp-idf" }),
    ).toBe("v6.1 at /home/u/esp/esp-idf");
  });

  it("runs the arduino-cli installer, shows its log and re-probes", async () => {
    setupInstallArduinoCli.mockResolvedValue({
      ok: true,
      bindir: "/home/u/.local/bin",
      log: "$ curl …\nInstalled arduino-cli 1.4.0\n",
    });
    const notify = vi.fn();
    render(<SetupPanel onClose={() => {}} notify={notify} />);
    const cli = await screen.findByTestId("setup-tool-arduino-cli");
    fireEvent.click(cli.querySelector("button.primary")!);
    await waitFor(() => expect(setupInstallArduinoCli).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByTestId("setup-install-log").textContent).toContain(
        "Installed arduino-cli 1.4.0",
      ),
    );
    expect(notify).toHaveBeenCalledWith(
      "arduino-cli installed into /home/u/.local/bin",
      false,
    );
    // Mount probe + the re-probe after installing.
    expect(setupProbe).toHaveBeenCalledTimes(2);
  });

  it("a failed install says so and keeps the log visible", async () => {
    setupInstallArduinoCli.mockResolvedValue({
      ok: false,
      bindir: "/home/u/.local/bin",
      log: "$ curl …\ncurl: (6) Could not resolve host\n(exited with status 6)\n",
    });
    const notify = vi.fn();
    render(<SetupPanel onClose={() => {}} notify={notify} />);
    const cli = await screen.findByTestId("setup-tool-arduino-cli");
    fireEvent.click(cli.querySelector("button.primary")!);
    await waitFor(() =>
      expect(notify).toHaveBeenCalledWith(
        "arduino-cli install failed — see the log below",
        true,
      ),
    );
    expect(screen.getByTestId("setup-install-log").textContent).toContain(
      "Could not resolve host",
    );
  });

  it("⟳ probes again and Close closes", async () => {
    const onClose = vi.fn();
    render(<SetupPanel onClose={onClose} notify={() => {}} />);
    await screen.findByTestId("setup-tool-git");
    fireEvent.click(screen.getByLabelText("Check again"));
    await waitFor(() => expect(setupProbe).toHaveBeenCalledTimes(2));
    fireEvent.click(screen.getByText("Close"));
    expect(onClose).toHaveBeenCalled();
  });

  it("words a present-but-broken tool differently from a missing one", () => {
    expect(toolState(tool({ ok: false, detail: "exited with status 1" }))).toBe(
      "installed but not working",
    );
    expect(toolState(tool({ ok: false, path: "/usr/bin/gh" }))).toBe(
      "found at /usr/bin/gh but not runnable",
    );
    expect(toolState(tool({ ok: false }))).toBe("not found");
    expect(toolState(tool({ ok: true, version: "" }))).toBe("installed");
  });
});
