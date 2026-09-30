// Bill of Materials panel: a table editor for bom.yaml in the sketch directory.
//
// Always mounted, hidden with `display:none` when another tab is active (same
// pattern as SerialMonitor) so the table state persists across tab switches.
// The BOM is a plain YAML file — the Agent can read and edit it, and it
// travels with the project in git.

import { useEffect, useRef, useState } from "react";
import { loadBom, saveBom, type BomEntry } from "../api";

interface Props {
  active: boolean;
  sketchDir: string | null;
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
});

/** Coerce the row to the wire shape: drop empty optional strings → null. */
const clean = (e: BomEntry): BomEntry => ({
  qty: Number(e.qty) || 1,
  ref: e.ref,
  value: e.value,
  package:  e.package  || null,
  supplier: e.supplier || null,
  part_no:  e.part_no  || null,
  notes:    e.notes    || null,
});

export default function BomPanel({ active, sketchDir, notify }: Props) {
  /** null = not loaded yet; [] = loaded, no file; BomEntry[] = file loaded */
  const [rows, setRows] = useState<BomEntry[] | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saving, setSaving] = useState(false);
  const addRowRef = useRef<HTMLButtonElement>(null);

  // Reload whenever the open project changes.
  useEffect(() => {
    if (!sketchDir) {
      setRows(null);
      setDirty(false);
      return;
    }
    setRows(null);
    loadBom(sketchDir)
      .then((bom) => {
        if (bom) {
          // Expand null optionals to "" so inputs are controlled.
          setRows(
            bom.components.map((e) => ({
              qty:      e.qty,
              ref:      e.ref,
              value:    e.value,
              package:  e.package  ?? "",
              supplier: e.supplier ?? "",
              part_no:  e.part_no  ?? "",
              notes:    e.notes    ?? "",
            })),
          );
        } else {
          setRows([]);   // file absent → empty state (not null)
        }
        setDirty(false);
      })
      .catch((err) => notify(String(err), true));
  }, [sketchDir]);

  const update = (i: number, field: keyof BomEntry, val: string | number) => {
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
    setDirty(true);
  };

  const save = async () => {
    if (!sketchDir || !rows) return;
    setSaving(true);
    try {
      await saveBom(sketchDir, { components: rows.map(clean) });
      setDirty(false);
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
      // Focus the first cell of the new row on the next paint.
      requestAnimationFrame(() => {
        const table = (e.target as HTMLElement).closest("table");
        const newRow = table?.querySelectorAll("tbody tr")[rowIdx + 1];
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
              {COLS.map((c) => (
                <col key={c.key} style={{ width: c.width }} />
              ))}
              <col style={{ width: "2rem" }} />
            </colgroup>
            <thead>
              <tr>
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
                <tr key={ri} className="bom-row">
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
