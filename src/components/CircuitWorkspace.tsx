import { useEffect, useMemo, useState } from "react";

import * as api from "../api";
import {
  addComponent,
  addConnection,
  newCircuitManifest,
  selectCircuitBoard,
} from "../circuitModel";

interface Props {
  sketchDir: string;
  profile: string | null;
  fqbn: string | null;
  onClose: () => void;
  onFilesChanged: () => void;
  onValidation: (validation: api.CircuitValidation | null) => void;
  notify: (message: string, error?: boolean) => void;
}

const projectName = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) ?? "Project";
const numberOrNull = (value: string): number | null =>
  value.trim() === "" || Number.isNaN(Number(value)) ? null : Number(value);

export default function CircuitWorkspace({
  sketchDir,
  profile,
  fqbn,
  onClose,
  onFilesChanged,
  onValidation,
  notify,
}: Props) {
  const [catalog, setCatalog] = useState<api.CircuitCatalog | null>(null);
  const [draft, setDraft] = useState<api.CircuitManifest | null>(null);
  const [snapshot, setSnapshot] = useState<api.CircuitSnapshot | null>(null);
  const [componentKind, setComponentKind] = useState("generic");
  const [busy, setBusy] = useState(true);
  const [dirty, setDirty] = useState(false);
  const [mouserStatus, setMouserStatus] = useState<api.MouserConfigStatus>({ configured: false });
  const [mouserKey, setMouserKey] = useState("");
  const [mouserQuery, setMouserQuery] = useState("");
  const [mouserKind, setMouserKind] = useState<api.MouserSearchKind>("keyword");
  const [mouserOption, setMouserOption] = useState<api.MouserSearchOption>("in_stock");
  const [mouserTarget, setMouserTarget] = useState(0);
  const [mouserResults, setMouserResults] = useState<api.MouserSearchResponse | null>(null);
  const [mouserBusy, setMouserBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    setBusy(true);
    setMouserResults(null);
    setMouserQuery("");
    setMouserTarget(0);
    setMouserKind("keyword");
    setMouserOption("in_stock");
    Promise.all([api.circuitCatalog(), api.circuitLoad(sketchDir, profile ?? undefined, fqbn ?? undefined)])
      .then(([nextCatalog, loaded]) => {
        if (!alive) return;
        setCatalog(nextCatalog);
        setSnapshot(loaded);
        setDraft(
          loaded?.manifest ??
            newCircuitManifest(nextCatalog, projectName(sketchDir), fqbn ?? undefined),
        );
        setComponentKind(nextCatalog.component_kinds[0]?.kind ?? "generic");
        onValidation(loaded?.validation ?? null);
        setDirty(false);
      })
      .catch((error) => notify(String(error), true))
      .finally(() => alive && setBusy(false));
    api.mouserConfigStatus()
      .then((status) => alive && setMouserStatus(status))
      .catch((error) => alive && notify(`Mouser configuration: ${String(error)}`, true));
    return () => {
      alive = false;
    };
  }, [sketchDir, profile, fqbn]);

  const board = useMemo(
    () => catalog?.boards.find((item) => item.id === draft?.board.catalog_id),
    [catalog, draft?.board.catalog_id],
  );
  const update = (next: api.CircuitManifest) => {
    setDraft(next);
    setDirty(true);
  };

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    try {
      const next = await api.circuitSave(
        sketchDir,
        draft,
        profile ?? undefined,
        fqbn ?? undefined,
      );
      setSnapshot(next);
      setDraft(next.manifest);
      setDirty(false);
      onValidation(next.validation);
      onFilesChanged();
      notify(next.validation.valid ? "Circuit synchronized" : "Circuit saved with blocking issues", !next.validation.valid);
    } catch (error) {
      notify(String(error), true);
    } finally {
      setBusy(false);
    }
  };

  const saveMouserKey = async () => {
    setMouserBusy(true);
    try {
      const status = await api.mouserSetApiKey(mouserKey);
      setMouserStatus(status);
      setMouserKey("");
      notify("Mouser API key saved in the private application credentials file");
    } catch (error) {
      notify(String(error), true);
    } finally {
      setMouserBusy(false);
    }
  };

  const clearMouserKey = async () => {
    setMouserBusy(true);
    try {
      const status = await api.mouserClearApiKey();
      setMouserStatus(status);
      setMouserResults(null);
      notify(status.configured ? "Private key removed; environment key is still active" : "Mouser API key removed");
    } catch (error) {
      notify(String(error), true);
    } finally {
      setMouserBusy(false);
    }
  };

  const searchMouser = async () => {
    setMouserBusy(true);
    try {
      const result = await api.mouserSearch({
        query: mouserQuery,
        kind: mouserKind,
        records: 10,
        starting_record: 0,
        option: mouserOption,
        exact: mouserKind === "part_number",
      });
      setMouserResults(result);
      if (result.parts.length === 0) notify("Mouser returned no matching parts");
    } catch (error) {
      setMouserResults(null);
      notify(String(error), true);
    } finally {
      setMouserBusy(false);
    }
  };

  if (busy && !draft) return <div className="circuit-workspace empty">Loading circuit…</div>;
  if (!catalog || !draft) return <div className="circuit-workspace empty">Circuit data unavailable.</div>;

  const diagnostics = snapshot?.validation.diagnostics ?? [];
  const svgUrl = snapshot
    ? `data:image/svg+xml;charset=utf-8,${encodeURIComponent(snapshot.svg)}`
    : null;

  return (
    <div className="circuit-workspace">
      <header className="circuit-header">
        <div>
          <h2>Project circuit</h2>
          <p>
            <code>hardware/circuit.yaml</code> drives the pin header, wiring diagram,
            BOM, and build guard.
          </p>
        </div>
        <div className="circuit-actions">
          <button onClick={onClose}>Back to code</button>
          <button className="primary" disabled={busy || !dirty} onClick={() => void save()}>
            {snapshot ? "Save & synchronize" : "Create circuit"}
          </button>
        </div>
      </header>

      <div className="circuit-status-row">
        <span className={`circuit-badge ${snapshot?.validation.valid ? "valid" : "blocked"}`}>
          {!snapshot ? "Not created" : snapshot.validation.valid ? "Build ready" : "Build blocked"}
        </span>
        {snapshot && <code>{snapshot.validation.manifest_digest}</code>}
        {dirty && <span className="circuit-badge dirty">Unsaved</span>}
      </div>

      <div className="circuit-grid">
        <section className="circuit-card">
          <h3>Board</h3>
          <label>
            Circuit name
            <input value={draft.name} onChange={(event) => update({ ...draft, name: event.target.value })} />
          </label>
          <label>
            Development board
            <select
              value={draft.board.catalog_id}
              onChange={(event) => update(selectCircuitBoard(draft, catalog, event.target.value))}
            >
              {catalog.boards.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}
            </select>
          </label>
          {board && (
            <p className="circuit-hint">
              Logic {board.logic_voltage_v} V · 3V3 budget {board.max_3v3_ma} mA ·{" "}
              <a href={board.source_url} target="_blank" rel="noreferrer">Espressif reference</a>
            </p>
          )}
        </section>

        <section className="circuit-card circuit-mouser">
          <div className="circuit-card-title">
            <h3>Mouser live lookup</h3>
            <span className={`circuit-badge ${mouserStatus.configured ? "valid" : "neutral"}`}>
              {mouserStatus.configured
                ? mouserStatus.source === "environment" ? "MOUSER_API_KEY" : "API configured"
                : "Not configured"}
            </span>
          </div>
          {!mouserStatus.configured || mouserStatus.source !== "environment" ? (
            <div className="mouser-key-row">
              <input
                aria-label="Mouser API key"
                type="password"
                autoComplete="off"
                placeholder={mouserStatus.configured ? "Replace API key" : "Search API key"}
                value={mouserKey}
                onChange={(event) => setMouserKey(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && mouserKey.trim()) void saveMouserKey();
                }}
              />
              <button disabled={mouserBusy || !mouserKey.trim()} onClick={() => void saveMouserKey()}>Save key</button>
              {mouserStatus.source === "private_file" && (
                <button className="danger subtle" disabled={mouserBusy} onClick={() => void clearMouserKey()}>Clear</button>
              )}
            </div>
          ) : (
            <p className="circuit-hint">The key comes from the process environment and is never exposed to the webview.</p>
          )}
          <p className="circuit-hint">
            Request a Search API key from <a href="https://www.mouser.com/api-search/" target="_blank" rel="noreferrer">Mouser</a>.
            Live results are not written to <code>circuit.yaml</code> or the BOM. Review the <a href="https://www.mouser.com/apiterms/" target="_blank" rel="noreferrer">API terms</a>.
          </p>
          <div className="mouser-search-row">
            <select
              aria-label="Seed Mouser search from component"
              value={Math.min(mouserTarget, Math.max(0, draft.components.length - 1))}
              disabled={draft.components.length === 0}
              onChange={(event) => {
                const index = Number(event.target.value);
                setMouserTarget(index);
                const component = draft.components[index];
                setMouserQuery(component?.part_number || component?.label || "");
                setMouserKind(component?.part_number ? "part_number" : "keyword");
                if (component?.part_number) setMouserOption("none");
              }}
            >
              {draft.components.length === 0
                ? <option value={0}>No project components</option>
                : draft.components.map((component, index) => <option key={`${component.id}:${index}`} value={index}>{component.label || component.id}</option>)}
            </select>
            <select aria-label="Mouser search kind" value={mouserKind} onChange={(event) => {
              const kind = event.target.value as api.MouserSearchKind;
              setMouserKind(kind);
              if (kind === "part_number") setMouserOption("none");
            }}>
              <option value="keyword">Keyword</option>
              <option value="part_number">Part number</option>
            </select>
            <input
              aria-label="Mouser search query"
              placeholder="Manufacturer part number or keywords"
              value={mouserQuery}
              onChange={(event) => setMouserQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && mouserStatus.configured && mouserQuery.trim()) void searchMouser();
              }}
            />
            <select aria-label="Mouser search filter" value={mouserOption} disabled={mouserKind === "part_number"} onChange={(event) => setMouserOption(event.target.value as api.MouserSearchOption)}>
              <option value="none">All</option>
              <option value="in_stock">In stock</option>
              <option value="rohs">RoHS</option>
              <option value="rohs_and_in_stock">RoHS and in stock</option>
            </select>
            <button className="primary" disabled={mouserBusy || !mouserStatus.configured || !mouserQuery.trim()} onClick={() => void searchMouser()}>
              {mouserBusy ? "Searching…" : "Search"}
            </button>
          </div>
          {mouserResults && (
            <div className="mouser-results">
              <p className="circuit-hint">
                Showing {mouserResults.parts.length} of {mouserResults.total} matches. Live data provided by Mouser Electronics.
              </p>
              {mouserResults.parts.map((part) => {
                const price = part.price_breaks[0];
                return (
                  <article className="mouser-result" key={part.mouser_part_number}>
                    <div>
                      <strong>{part.manufacturer_part_number || part.mouser_part_number}</strong>
                      <span>{part.manufacturer}</span>
                      <p>{part.description}</p>
                    </div>
                    <div className="mouser-facts">
                      <span>{part.availability || "Availability unknown"}</span>
                      <span>{part.lifecycle_status || "Lifecycle unknown"}</span>
                      {price && <span>{price.price} {price.currency} at {price.quantity}+</span>}
                    </div>
                    <div className="mouser-links">
                      {part.datasheet_url && <a href={part.datasheet_url} target="_blank" rel="noreferrer">Datasheet</a>}
                      {part.product_url && <a href={part.product_url} target="_blank" rel="noreferrer">View at Mouser</a>}
                    </div>
                  </article>
                );
              })}
            </div>
          )}
        </section>

        <section className="circuit-card circuit-components">
          <div className="circuit-card-title">
            <h3>Components</h3>
            <div>
              <select value={componentKind} onChange={(event) => setComponentKind(event.target.value)}>
                {catalog.component_kinds.map((kind) => <option key={kind.kind} value={kind.kind}>{kind.label}</option>)}
              </select>
              <button onClick={() => {
                const template = catalog.component_kinds.find((item) => item.kind === componentKind);
                if (template) update(addComponent(draft, template));
              }}>+ Add</button>
            </div>
          </div>
          {draft.components.length === 0 && <p className="circuit-hint">Add every powered or signal-connected part.</p>}
          {draft.components.map((component, index) => {
            const template = catalog.component_kinds.find((item) => item.kind === component.kind);
            const patchComponent = (patch: Partial<api.CircuitComponent>) => {
              const components = [...draft.components];
              components[index] = { ...component, ...patch };
              update({ ...draft, components });
            };
            const setProperty = (key: string, value: string) =>
              patchComponent({ properties: { ...component.properties, [key]: value } });
            return (
              <fieldset key={`${component.id}:${index}`} className="circuit-item">
                <legend>{component.label || component.id}</legend>
                <div className="circuit-fields">
                  <label>ID<input value={component.id} onChange={(e) => patchComponent({ id: e.target.value })} /></label>
                  <label>Label<input value={component.label} onChange={(e) => patchComponent({ label: e.target.value })} /></label>
                  <label>Part number<input value={component.part_number ?? ""} onChange={(e) => patchComponent({ part_number: e.target.value || null })} /></label>
                  <label>Voltage (V)<input type="number" step="0.1" value={component.voltage_v ?? ""} onChange={(e) => patchComponent({ voltage_v: numberOrNull(e.target.value) })} /></label>
                  <label>Current (mA)<input type="number" value={component.current_ma ?? ""} onChange={(e) => patchComponent({ current_ma: numberOrNull(e.target.value) })} /></label>
                  <label>Pins<input value={component.pins.map((pin) => pin.id).join(", ")} onChange={(e) => patchComponent({ pins: e.target.value.split(",").map((pin) => pin.trim()).filter(Boolean).map((pin) => ({ id: pin, label: pin })) })} /></label>
                </div>
                <label className="circuit-check"><input type="checkbox" checked={component.verified} onChange={(e) => patchComponent({ verified: e.target.checked })} /> Datasheet pinout and ratings verified</label>
                {component.kind === "led" && <label>Series resistor (Ω)<input type="number" value={component.properties.series_resistor_ohms ?? ""} onChange={(e) => setProperty("series_resistor_ohms", e.target.value)} /></label>}
                {component.kind === "i2c_sensor" && <label>I²C pull-ups<select value={component.properties.pullups ?? "unknown"} onChange={(e) => setProperty("pullups", e.target.value)}><option value="unknown">Not confirmed</option><option value="onboard">On module</option><option value="external">External</option></select></label>}
                {(component.kind === "relay" || component.kind === "motor") && <div className="circuit-checks"><label><input type="checkbox" checked={component.properties.driver === "confirmed"} onChange={(e) => setProperty("driver", e.target.checked ? "confirmed" : "")}/> Rated driver</label><label><input type="checkbox" checked={component.properties.flyback_protection === "confirmed"} onChange={(e) => setProperty("flyback_protection", e.target.checked ? "confirmed" : "")}/> Flyback protection</label></div>}
                {template && <p className="circuit-hint">{template.safety_hint}</p>}
                <button className="danger subtle" onClick={() => update({ ...draft, components: draft.components.filter((_, i) => i !== index), connections: draft.connections.filter((wire) => wire.from.target !== component.id && wire.to.target !== component.id) })}>Remove component</button>
              </fieldset>
            );
          })}
        </section>

        <section className="circuit-card circuit-connections">
          <div className="circuit-card-title">
            <h3>Connections</h3>
            <button disabled={draft.components.length === 0} onClick={() => update(addConnection(draft))}>+ Add wire</button>
          </div>
          {draft.connections.map((wire, index) => {
            const component = draft.components.find((item) => item.id === wire.to.target) ?? draft.components[0];
            const patchWire = (patch: Partial<api.CircuitConnection>) => {
              const connections = [...draft.connections];
              connections[index] = { ...wire, ...patch };
              update({ ...draft, connections });
            };
            return (
              <div className="circuit-wire" key={`${wire.id}:${index}`}>
                <input aria-label="Connection id" value={wire.id} onChange={(e) => patchWire({ id: e.target.value })} />
                <select aria-label="Board pin" value={wire.from.pin} onChange={(e) => patchWire({ from: { target: "board", pin: e.target.value } })}>
                  {board?.pins.map((pin) => <option key={pin.id} value={pin.id}>{pin.label}{pin.strapping ? " ⚠ strap" : ""}{pin.reserved ? " ⛔ reserved" : ""}</option>)}
                </select>
                <span>→</span>
                <select aria-label="Component" value={wire.to.target} onChange={(e) => {
                  const next = draft.components.find((item) => item.id === e.target.value);
                  patchWire({ to: { target: e.target.value, pin: next?.pins[0]?.id ?? "pin" } });
                }}>{draft.components.map((item) => <option key={item.id} value={item.id}>{item.label}</option>)}</select>
                <select aria-label="Component pin" value={wire.to.pin} onChange={(e) => patchWire({ to: { ...wire.to, pin: e.target.value } })}>{component?.pins.map((pin) => <option key={pin.id} value={pin.id}>{pin.label || pin.id}</option>)}</select>
                <select aria-label="Role" value={wire.role} onChange={(e) => patchWire({ role: e.target.value })}><option value="ground">Ground</option><option value="power">Power</option><option value="signal">Bidirectional signal</option><option value="input">Digital input</option><option value="output">Digital output</option><option value="pwm">PWM output</option><option value="adc">Analog input</option><option value="i2c_sda">I²C SDA</option><option value="i2c_scl">I²C SCL</option><option value="spi">SPI</option><option value="uart">UART</option></select>
                <input aria-label="Voltage" type="number" step="0.1" placeholder="V" value={wire.voltage_v ?? ""} onChange={(e) => patchWire({ voltage_v: numberOrNull(e.target.value) })} />
                <input aria-label="Firmware symbol" placeholder="PIN_NAME" value={wire.firmware_symbol ?? ""} disabled={!wire.from.pin.startsWith("GPIO")} onChange={(e) => patchWire({ firmware_symbol: e.target.value || null })} />
                <button aria-label="Remove wire" className="danger subtle" onClick={() => update({ ...draft, connections: draft.connections.filter((_, i) => i !== index) })}>×</button>
              </div>
            );
          })}
        </section>

        <section className="circuit-card circuit-preview">
          <h3>Generated current state</h3>
          {svgUrl ? <img src={svgUrl} alt={`${draft.name} wiring diagram`} /> : <p className="circuit-hint">Save the manifest to generate the architecture diagram and supporting artifacts.</p>}
        </section>

        <section className="circuit-card circuit-validation">
          <h3>Validation</h3>
          {diagnostics.length === 0 ? <p className="circuit-ok">No electrical or firmware-sync issues.</p> : diagnostics.map((item, index) => (
            <div key={`${item.code}:${index}`} className={`circuit-diagnostic ${item.severity}`}><code>{item.code}</code><span>{item.message}</span></div>
          ))}
          {snapshot && <details><summary>Generated artifacts</summary><ul>{snapshot.validation.artifacts.map((item) => <li key={item.path}>{item.current ? "✓" : "✗"} <code>{item.path}</code></li>)}</ul></details>}
        </section>
      </div>
    </div>
  );
}
