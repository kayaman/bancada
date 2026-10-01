// Bill of Materials panel: a table editor for bom.yaml in the sketch directory.
//
// Always mounted, hidden with `display:none` when another tab is active (same
// pattern as SerialMonitor) so the table state persists across tab switches.
// The BOM is a plain YAML file — the Agent can read and edit it, and it
// travels with the project in git.

import { useEffect, useRef, useState } from "react";
import { loadBom, saveBom, type BomEntry, type WiringEntry } from "../api";

interface Props {
  active: boolean;
  sketchDir: string | null;
  bomVersion?: number;
  onSaved: () => void;
  notify: (msg: string, isError?: boolean) => void;
}

const COLS: { key: keyof BomEntry; label: string; width: string; numeric?: boolean }[] = [
  { key: "qty",      label: "Qty",      width: "4rem",  numeric: true },
  { key: "ref",      label: "Ref",      width: "7rem"  },
  { key: "value",    label: "Value",    width: "12rem" },
  { key: "package",  label: "Package",  width: "6rem"  },
  { key: "supplier", label: "Supplier", width: "7rem"  },
  { key: "part_no",  label: "Part No",  width: "9rem"  },
  { key: "notes",    label: "Notes",    width: "1fr"   },
];

const blank = (): BomEntry => ({
  qty: 1,
  ref: "",
  value: "",
  package: "",
  supplier: "",
  part_no: "",
  notes: "",
  description: "",
  images: [],
  wiring: [],
});

const blankWire = (): WiringEntry => ({ pin: "", gpio: null, rail: "", notes: "" });

/** Coerce the row to the wire shape: drop empty optional strings → null. */
const cleanWire = (w: WiringEntry): WiringEntry => ({
  pin: w.pin,
  gpio: w.gpio ?? null,
  rail: w.rail || null,
  notes: w.notes || null,
});

const clean = (e: BomEntry): BomEntry => ({
  qty: Number(e.qty) || 1,
  ref: e.ref,
  value: e.value,
  package:     e.package     || null,
  supplier:    e.supplier    || null,
  part_no:     e.part_no     || null,
  notes:       e.notes       || null,
  description: e.description || null,
  images:      (e.images ?? []).filter(Boolean),
  wiring:      (e.wiring ?? []).filter((w) => w.pin).map(cleanWire),
});

export default function BomPanel({ active, sketchDir, bomVersion = 0, onSaved, notify }: Props) {
  /** null = not loaded yet; [] = loaded, no file; BomEntry[] = file loaded */
  const [rows, setRows] = useState<BomEntry[] | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const addRowRef = useRef<HTMLButtonElement>(null);

  // Reload whenever the open project changes or the agent writes bom.yaml.
  useEffect(() => {
    if (!sketchDir) {
      setRows(null);
      setDirty(false);
      setExpanded(new Set());
      return;
    }
    setRows(null);
    setExpanded(new Set());
    loadBom(sketchDir)
      .then((bom) => {
        if (bom) {
          setRows(
            bom.components.map((e) => ({
              qty:         e.qty,
              ref:         e.ref,
              value:       e.value,
              package:     e.package     ?? "",
              supplier:    e.supplier    ?? "",
              part_no:     e.part_no     ?? "",
              notes:       e.notes       ?? "",
              description: e.description ?? "",
              images:      e.images      ?? [],
              wiring:      (e.wiring ?? []).map((w) => ({
                pin:   w.pin,
                gpio:  w.gpio  ?? null,
                rail:  w.rail  ?? "",
                notes: w.notes ?? "",
              })),
            })),
          );
        } else {
          setRows([]);
        }
        setDirty(false);
      })
      .catch((err) => notify(String(err), true));
  }, [sketchDir, bomVersion]);

  const update = (i: number, field: keyof BomEntry, val: unknown) => {
    setRows((prev) => {
      if (!prev) return prev;
      const next = [...prev];
      next[i] = { ...next[i], [field]: val };
      return next;
    });
    setDirty(true);
  };

  const addRow = () => {
    setRows((prev) => (prev ? [...prev, blank()] : [blank()]));
    setDirty(true);
  };

  const deleteRow = (i: number) => {
    setRows((prev) => {
      if (!prev) return prev;
      const next = [...prev];
      next.splice(i, 1);
      return next;
    });
    setExpanded((prev) => {
      const next = new Set<number>();
      for (const idx of prev) if (idx !== i) next.add(idx > i ? idx - 1 : idx);
      return next;
    });
    setDirty(true);
  };

  const toggleExpand = (i: number) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(i)) next.delete(i); else next.add(i);
      return next;
    });
  };

  // Wiring helpers
  const addWire = (rowIdx: number) => {
    const row = rows?.[rowIdx];
    if (!row) return;
    update(rowIdx, "wiring", [...(row.wiring ?? []), blankWire()]);
  };

  const updateWire = (rowIdx: number, wireIdx: number, field: keyof WiringEntry, val: string | number | null) => {
    const row = rows?.[rowIdx];
    if (!row) return;
    const wires = [...(row.wiring ?? [])];
    wires[wireIdx] = { ...wires[wireIdx], [field]: val };
    update(rowIdx, "wiring", wires);
  };

  const deleteWire = (rowIdx: number, wireIdx: number) => {
    const row = rows?.[rowIdx];
    if (!row) return;
    const wires = [...(row.wiring ?? [])];
    wires.splice(wireIdx, 1);
    update(rowIdx, "wiring", wires);
  };

  // Image helpers
  const addImage = (rowIdx: number, url: string) => {
    const row = rows?.[rowIdx];
    if (!row || !url.trim()) return;
    update(rowIdx, "images", [...(row.images ?? []), url.trim()]);
  };

  const deleteImage = (rowIdx: number, imgIdx: number) => {
    const row = rows?.[rowIdx];
    if (!row) return;
    const imgs = [...(row.images ?? [])];
    imgs.splice(imgIdx, 1);
    update(rowIdx, "images", imgs);
  };

  const save = async () => {
    if (!sketchDir || !rows) return;
    setSaving(true);
    try {
      await saveBom(sketchDir, { components: rows.map(clean) });
      setDirty(false);
      onSaved();
      notify("BOM saved");
    } catch (err) {
      notify(String(err), true);
    } finally {
      setSaving(false);
    }
  };

  // Tab to next cell; Enter on the last cell of a row adds a new row.
  const onCellKeyDown = (
    e: React.KeyboardEvent<HTMLInputElement>,
    rowIdx: number,
    colIdx: number,
  ) => {
    if (e.key === "Enter" && colIdx === COLS.length - 1) {
      e.preventDefault();
      addRow();
      requestAnimationFrame(() => {
        const table = (e.target as HTMLElement).closest("table");
        const newRow = table?.querySelectorAll("tbody tr.bom-row")[rowIdx + 1];
        (newRow?.querySelector("input") as HTMLInputElement | null)?.focus();
      });
    }
  };

  const totalQty = rows?.reduce((s, r) => s + (Number(r.qty) || 0), 0) ?? 0;

  return (
    <section
      className="bom-panel"
      style={active ? undefined : { display: "none" }}
    >
      <div className="bom-toolbar">
        {rows !== null && rows.length > 0 && (
          <span className="bom-count">
            {rows.length} line{rows.length === 1 ? "" : "s"} · {totalQty} parts
          </span>
        )}
        <div className="spacer" />
        {rows !== null && rows.length >= 0 && (
          <button
            className={dirty ? "btn small primary" : "btn small"}
            disabled={saving || !dirty || !sketchDir}
            onClick={() => void save()}
            title={dirty ? "Save bom.yaml to disk" : "No unsaved changes"}
          >
            {saving ? "Saving…" : "Save"}
          </button>
        )}
      </div>

      {rows === null && (
        <div className="bom-empty">Loading…</div>
      )}

      {rows !== null && rows.length === 0 && !dirty && (
        <div className="bom-empty">
          <p>No <code>bom.yaml</code> in this project yet.</p>
          <button className="btn primary" onClick={addRow} disabled={!sketchDir}>
            Create BOM
          </button>
        </div>
      )}

      {rows !== null && (rows.length > 0 || dirty) && (
        <div className="bom-scroll">
          <table className="bom-table">
            <colgroup>
              <col style={{ width: "1.5rem" }} />
              {COLS.map((c) => (
                <col key={c.key} style={{ width: c.width }} />
              ))}
              <col style={{ width: "2rem" }} />
            </colgroup>
            <thead>
              <tr>
                <th className="bom-th" />
                {COLS.map((c) => (
                  <th key={c.key} className="bom-th">
                    {c.label}
                  </th>
                ))}
                <th className="bom-th" />
              </tr>
            </thead>
            <tbody>
              {rows.map((row, ri) => (
                <>
                  <tr key={`row-${ri}`} className="bom-row">
                    <td className="bom-td bom-td-expand">
                      <button
                        className="bom-row-expand"
                        title={expanded.has(ri) ? "Collapse details" : "Expand details"}
                        onClick={() => toggleExpand(ri)}
                      >
                        {expanded.has(ri) ? "▾" : "▸"}
                      </button>
                    </td>
                    {COLS.map((col, ci) => (
                      <td key={col.key} className="bom-td">
                        <input
                          className="bom-cell"
                          type={col.numeric ? "number" : "text"}
                          min={col.numeric ? 1 : undefined}
                          value={String(row[col.key] ?? "")}
                          onChange={(e) =>
                            update(
                              ri,
                              col.key,
                              col.numeric ? Number(e.target.value) : e.target.value,
                            )
                          }
                          onKeyDown={(e) => onCellKeyDown(e, ri, ci)}
                        />
                      </td>
                    ))}
                    <td className="bom-td bom-td-delete">
                      <button
                        className="bom-delete"
                        title="Remove row"
                        onClick={() => deleteRow(ri)}
                      >
                        ✕
                      </button>
                    </td>
                  </tr>
                  {expanded.has(ri) && (
                    <tr key={`detail-${ri}`} className="bom-detail">
                      <td colSpan={COLS.length + 2} className="bom-detail-cell">
                        <div className="bom-detail-inner">

                          <div className="bom-detail-section">
                            <span className="bom-detail-label">Description</span>
                            <textarea
                              className="bom-desc"
                              rows={2}
                              value={row.description ?? ""}
                              placeholder="What does this component do?"
                              onChange={(e) => update(ri, "description", e.target.value)}
                            />
                          </div>

                          <div className="bom-detail-section">
                            <span className="bom-detail-label">Images</span>
                            <div className="bom-images-list">
                              {(row.images ?? []).map((url, ii) => (
                                <span key={ii} className="bom-image-chip">
                                  <a href={url} target="_blank" rel="noreferrer" className="bom-image-url">{url}</a>
                                  <button className="bom-image-del" onClick={() => deleteImage(ri, ii)}>✕</button>
                                </span>
                              ))}
                              <ImageInput onAdd={(url) => addImage(ri, url)} />
                            </div>
                          </div>

                          <div className="bom-detail-section">
                            <span className="bom-detail-label">Wiring</span>
                            {(row.wiring ?? []).length > 0 && (
                              <table className="bom-wiring-table">
                                <thead>
                                  <tr>
                                    <th className="bom-wth">Pin</th>
                                    <th className="bom-wth">GPIO</th>
                                    <th className="bom-wth">Rail</th>
                                    <th className="bom-wth">Notes</th>
                                    <th className="bom-wth" />
                                  </tr>
                                </thead>
                                <tbody>
                                  {(row.wiring ?? []).map((w, wi) => (
                                    <tr key={wi} className="bom-wire-row">
                                      <td className="bom-wtd">
                                        <input className="bom-wcell" value={w.pin} placeholder="IO4"
                                          onChange={(e) => updateWire(ri, wi, "pin", e.target.value)} />
                                      </td>
                                      <td className="bom-wtd">
                                        <input className="bom-wcell" type="number" min={0}
                                          value={w.gpio ?? ""}
                                          placeholder="—"
                                          onChange={(e) => updateWire(ri, wi, "gpio", e.target.value === "" ? null : Number(e.target.value))} />
                                      </td>
                                      <td className="bom-wtd">
                                        <input className="bom-wcell" value={w.rail ?? ""} placeholder="3V3"
                                          onChange={(e) => updateWire(ri, wi, "rail", e.target.value)} />
                                      </td>
                                      <td className="bom-wtd">
                                        <input className="bom-wcell" value={w.notes ?? ""} placeholder="note"
                                          onChange={(e) => updateWire(ri, wi, "notes", e.target.value)} />
                                      </td>
                                      <td className="bom-wtd bom-wtd-del">
                                        <button className="bom-delete" title="Remove" onClick={() => deleteWire(ri, wi)}>✕</button>
                                      </td>
                                    </tr>
                                  ))}
                                </tbody>
                              </table>
                            )}
                            <button className="bom-add-wire" onClick={() => addWire(ri)}>
                              + Add connection
                            </button>
                          </div>

                        </div>
                      </td>
                    </tr>
                  )}
                </>
              ))}
            </tbody>
          </table>
          <button
            ref={addRowRef}
            className="bom-add-row"
            onClick={addRow}
          >
            + Add component
          </button>
        </div>
      )}
    </section>
  );
}

/** Controlled input for adding an image URL — clears itself after add. */
function ImageInput({ onAdd }: { onAdd: (url: string) => void }) {
  const [val, setVal] = useState("");
  const commit = () => {
    if (val.trim()) { onAdd(val.trim()); setVal(""); }
  };
  return (
    <span className="bom-image-add">
      <input
        className="bom-wcell"
        value={val}
        placeholder="https://…"
        onChange={(e) => setVal(e.target.value)}
        onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); commit(); } }}
      />
      <button className="btn small" onClick={commit} disabled={!val.trim()}>Add</button>
    </span>
  );
}
