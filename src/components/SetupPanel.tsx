// SetupPanel — the first-run checklist. Bancada bundles no toolchain; on a
// new machine that used to surface as a string of "could not find X on PATH"
// toasts, each true and none of them saying what to do. This panel is the
// answer: every engine the app drives, whether this machine has it, what it
// unlocks, and the command that installs it. arduino-cli — the one tool
// Arduino work cannot start without — can be installed from here; the rest
// are shown as commands to copy, because their installers are distro
// business (package managers, pip, sign-in flows) that a button would only
// get wrong.
//
// Probes on mount and on ⟳, never on a timer: each probe is one `--version`
// spawn per tool.

import { useEffect, useState } from "react";
import * as api from "../api";
import type { IdfProbe, InstallOutcome, SetupReport, ToolStatus } from "../api";

interface Props {
  onClose: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

/** Copy to the clipboard, degrading to a toast with the text when the
 *  clipboard is unavailable (a webview without the permission, jsdom). */
async function copyText(
  text: string,
  notify: Props["notify"],
): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    notify("Copied to clipboard");
  } catch {
    notify(text);
  }
}

/** One line summarising a tool's state, for the card header. */
export function toolState(t: ToolStatus): string {
  if (t.ok) return t.version || "installed";
  if (t.detail) return "installed but not working";
  if (t.path) return `found at ${t.path} but not runnable`;
  return t.required ? "not found — needed for Arduino projects" : "not found";
}

/** One line summarising ESP-IDF, which is probed separately from PATH tools. */
export function idfState(p: IdfProbe | null): string {
  if (!p) return "checking…";
  if (p.ok) return `${p.version ?? "installed"} at ${p.idf_path ?? "?"}`;
  return p.error ?? "not found";
}

export default function SetupPanel({ onClose, notify }: Props) {
  const [report, setReport] = useState<SetupReport | null>(null);
  const [idf, setIdf] = useState<IdfProbe | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [outcome, setOutcome] = useState<InstallOutcome | null>(null);

  const refresh = () => {
    setError(null);
    api
      .setupProbe()
      .then(setReport)
      .catch((e) => setError(String(e)));
    api
      .idfProbe()
      .then(setIdf)
      .catch((e) =>
        setIdf({ ok: false, error: String(e) }),
      );
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(refresh, []);

  const install = async () => {
    setInstalling(true);
    setOutcome(null);
    try {
      const r = await api.setupInstallArduinoCli();
      setOutcome(r);
      notify(
        r.ok
          ? `arduino-cli installed into ${r.bindir}`
          : "arduino-cli install failed — see the log below",
        !r.ok,
      );
      // Re-probe either way: a partial install is a state worth showing.
      refresh();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setInstalling(false);
    }
  };

  const serial = report?.serial;
  const serialOk = serial?.access.state === "ok";

  return (
    <div className="setup-panel" role="region" aria-label="Setup">
      <div className="setup-head">
        <span className="setup-title">🧰 Setup</span>
        <button
          className="btn icon"
          onClick={refresh}
          title="Check again"
          aria-label="Check again"
        >
          ⟳
        </button>
        <button className="btn" onClick={onClose}>
          Close
        </button>
      </div>
      <div className="setup-body">
        <p className="setup-intro">
          Bancada drives the same tools you would use in a terminal and bundles
          none of them. This is what this machine has. Only arduino-cli is
          needed for Arduino projects; an ESP-IDF-only bench needs only
          ESP-IDF.
        </p>
        {error && <div className="setup-detail">{error}</div>}

        {report?.tools.map((t) => (
          <div
            key={t.id}
            className={`setup-card ${t.ok ? "ok" : t.required ? "missing" : "optional"}`}
            data-testid={`setup-tool-${t.id}`}
          >
            <div className="setup-row">
              <span className="setup-name">
                {t.ok ? "✓" : "✗"} {t.name}
              </span>
              <span className={`setup-state${t.ok ? "" : " bad"}`}>
                {toolState(t)}
              </span>
            </div>
            <div className="setup-purpose">{t.purpose}</div>
            {t.detail && <div className="setup-detail">{t.detail}</div>}
            {!t.ok && (
              <div className="setup-cmd">
                <code>{t.install}</code>
                <button
                  className="btn"
                  onClick={() => void copyText(t.install, notify)}
                  title="Copy the install command"
                >
                  Copy
                </button>
                {t.installable && (
                  <button
                    className="btn primary"
                    onClick={() => void install()}
                    disabled={installing}
                    title="Run the official installer into ~/.local/bin"
                  >
                    {installing ? "Installing…" : "Install"}
                  </button>
                )}
              </div>
            )}
            {t.id === "arduino-cli" && outcome && (
              <pre className="setup-log" data-testid="setup-install-log">
                {outcome.log}
              </pre>
            )}
          </div>
        ))}

        <div
          className={`setup-card ${idf?.ok ? "ok" : "optional"}`}
          data-testid="setup-tool-esp-idf"
        >
          <div className="setup-row">
            <span className="setup-name">{idf?.ok ? "✓" : "✗"} ESP-IDF</span>
            <span className={`setup-state${idf && !idf.ok ? " bad" : ""}`}>
              {idfState(idf)}
            </span>
          </div>
          <div className="setup-purpose">
            ESP-IDF projects: build, flash, monitor. Found through the
            installer&apos;s registry or a manual install.sh checkout, never
            from IDF_PATH.
          </div>
          {idf && !idf.ok && (
            <div className="setup-cmd">
              <code>
                git clone -b v6.1 --recursive https://github.com/espressif/esp-idf.git
                ~/esp/esp-idf && ~/esp/esp-idf/install.sh
              </code>
              <button
                className="btn"
                onClick={() =>
                  void copyText(
                    "git clone -b v6.1 --recursive https://github.com/espressif/esp-idf.git ~/esp/esp-idf && ~/esp/esp-idf/install.sh",
                    notify,
                  )
                }
                title="Copy the install command"
              >
                Copy
              </button>
            </div>
          )}
        </div>

        {serial && (
          <div
            className={`setup-card ${serialOk ? "ok" : "missing"}`}
            data-testid="setup-serial"
          >
            <div className="setup-row">
              <span className="setup-name">
                {serialOk ? "✓" : "✗"} Serial-port access
              </span>
              <span className={`setup-state${serialOk ? "" : " bad"}`}>
                {serialOk
                  ? `member of ${serial.access.group}`
                  : `not in the ${serial.access.group} group`}
                {serial.device ? ` (owns ${serial.device})` : ""}
              </span>
            </div>
            <div className="setup-purpose">
              Flashing and the serial monitor open /dev/ttyACM* and
              /dev/ttyUSB* as your user.
            </div>
            {!serialOk && (
              <div className="setup-cmd">
                <code>{serial.fix}</code>
                <button
                  className="btn"
                  onClick={() => void copyText(serial.fix, notify)}
                  title="Copy the command"
                >
                  Copy
                </button>
              </div>
            )}
          </div>
        )}

        {report && (
          <div className="setup-path" title="The PATH these checks searched">
            PATH: {report.path}
          </div>
        )}
      </div>
    </div>
  );
}
