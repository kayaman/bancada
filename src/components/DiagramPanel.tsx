// Diagram panel — two sections:
//   A) Auto-rendered SVG wiring diagram from bom.yaml + board pin data
//   B) Agent-generated wiring.svg (if the file exists in the sketch dir)
//
// Always mounted, hidden with display:none when another tab is active.

import { useEffect, useState } from "react";
import {
  loadBom,
  projectInfo,
  readSketchFile,
  boardOf,
  type Bom,
  type Board,
  type WiringEntry,
} from "../api";

interface Props {
  active: boolean;
  sketchDir: string | null;
  bomVersion?: number;
  diagramVersion?: number;
  notify: (msg: string, isError?: boolean) => void;
}

// ── helpers ────────────────────────────────────────────────────────────────

/** Find the first board pin label for a given GPIO number. */
function gpioLabel(board: Board | null, gpio: number): string {
  if (!board) return `GPIO${gpio}`;
  for (const h of board.headers) {
    for (const p of h.pins) {
      if (p.gpio === gpio) return p.label;
    }
  }
  return `GPIO${gpio}`;
}

/** Colour a connection line by its target type. */
function wireColor(w: WiringEntry): string {
  if (w.gpio != null) return "#e07b39";          // orange — GPIO
  const r = (w.rail ?? "").toUpperCase();
  if (r === "GND") return "#555";                // dark grey — ground
  if (r.startsWith("3V") || r.startsWith("5V") || r.startsWith("VCC"))
    return "#c0392b";                            // crimson — power
  return "#4a90d9";                              // steel-blue — other rail
}

// ── auto-rendered wiring SVG ───────────────────────────────────────────────

interface Net {
  targetLabel: string;   // board-side label: "IO4" or "3V3"
  color: string;
}

interface ComponentRow {
  pinLabel: string;
  net: Net;
}

interface ComponentBlock {
  ref: string;
  value: string;
  rows: ComponentRow[];
}

const ROW_H = 28;
const BLOCK_GAP = 12;
const LEFT_W = 180;
const RIGHT_W = 100;
const SVG_W = 480;
const PAD_X = 16;
const PAD_Y = 16;

function buildBlocks(bom: Bom, board: Board | null): ComponentBlock[] {
  return bom.components
    .filter((c) => c.wiring && c.wiring.length > 0)
    .map((c) => ({
      ref: c.ref,
      value: c.value,
      rows: (c.wiring ?? []).map((w) => ({
        pinLabel: w.pin,
        net: {
          targetLabel: w.gpio != null ? gpioLabel(board, w.gpio) : (w.rail ?? "?"),
          color: wireColor(w),
        },
      })),
    }));
}

/** Collect unique net targets in stable order (first-seen). */
function buildTargets(blocks: ComponentBlock[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const b of blocks) {
    for (const r of b.rows) {
      if (!seen.has(r.net.targetLabel)) {
        seen.add(r.net.targetLabel);
        out.push(r.net.targetLabel);
      }
    }
  }
  return out;
}

function WiringDiagram({ bom, board }: { bom: Bom; board: Board | null }) {
  const blocks = buildBlocks(bom, board);
  if (blocks.length === 0) return null;

  const targets = buildTargets(blocks);

  // layout: left side — component rows stacked with gaps between blocks
  // compute Y for each left row
  const leftYs: number[] = [];
  let y = PAD_Y;
  for (const block of blocks) {
    y += ROW_H; // header row (component name)
    for (let i = 0; i < block.rows.length; i++) {
      leftYs.push(y + ROW_H / 2);
      y += ROW_H;
    }
    y += BLOCK_GAP;
  }

  // right side — target rows, spaced evenly from top
  const rightYs = new Map<string, number>();
  const rightTop = PAD_Y + ROW_H / 2;
  const rightStep = Math.max(ROW_H, (y - PAD_Y - BLOCK_GAP) / targets.length);
  targets.forEach((t, i) => rightYs.set(t, rightTop + i * rightStep));

  const svgH = Math.max(y, rightTop + targets.length * rightStep) + PAD_Y;
  const rightX = SVG_W - PAD_X - RIGHT_W;

  // flatten left rows for rendering
  const leftRows: { block: ComponentBlock; row: ComponentRow; y: number }[] = [];
  let li = 0;
  for (const block of blocks) {
    for (const row of block.rows) {
      leftRows.push({ block, row, y: leftYs[li++] });
    }
  }

  // recompute block header y positions
  const blockHeaderYs: { block: ComponentBlock; y: number }[] = [];
  {
    let yy = PAD_Y;
    for (const block of blocks) {
      blockHeaderYs.push({ block, y: yy + ROW_H / 2 + 4 });
      yy += ROW_H + block.rows.length * ROW_H + BLOCK_GAP;
    }
  }

  return (
    <svg
      width={SVG_W}
      height={svgH}
      className="diagram-auto-svg"
      aria-label="Wiring diagram"
    >
      {/* connection lines */}
      {leftRows.map(({ row, y: ly }, i) => {
        const ty = rightYs.get(row.net.targetLabel) ?? ly;
        const x0 = PAD_X + LEFT_W;
        const x1 = rightX;
        const cx = (x0 + x1) / 2;
        return (
          <path
            key={i}
            d={`M ${x0} ${ly} C ${cx} ${ly}, ${cx} ${ty}, ${x1} ${ty}`}
            fill="none"
            stroke={row.net.color}
            strokeWidth={1.5}
            opacity={0.7}
          />
        );
      })}

      {/* left side — component blocks */}
      {blockHeaderYs.map(({ block, y: hy }) => (
        <text
          key={block.ref}
          x={PAD_X}
          y={hy}
          className="diagram-comp-name"
        >
          {block.ref} {block.value}
        </text>
      ))}
      {leftRows.map(({ row, y: ly }, i) => (
        <g key={i}>
          <text x={PAD_X + 12} y={ly + 4} className="diagram-pin-label">
            {row.pinLabel}
          </text>
          <circle cx={PAD_X + LEFT_W} cy={ly} r={3} fill={row.net.color} />
        </g>
      ))}

      {/* right side — board targets */}
      {targets.map((t) => {
        const ty = rightYs.get(t) ?? 0;
        return (
          <g key={t}>
            <circle cx={rightX} cy={ty} r={3} fill="#888" />
            <text x={rightX + 8} y={ty + 4} className="diagram-pin-label diagram-board-pin">
              {t}
            </text>
          </g>
        );
      })}

      {/* column labels */}
      <text x={PAD_X} y={PAD_Y - 4} className="diagram-col-label">Components</text>
      <text x={rightX} y={PAD_Y - 4} className="diagram-col-label">Board</text>
    </svg>
  );
}

// ── panel ──────────────────────────────────────────────────────────────────

export default function DiagramPanel({
  active,
  sketchDir,
  bomVersion = 0,
  diagramVersion = 0,
  notify,
}: Props) {
  const [bom, setBom] = useState<Bom | null>(null);
  const [board, setBoard] = useState<Board | null>(null);
  const [svgContent, setSvgContent] = useState<string | null>(null);

  // Reload BOM whenever the project or agent edit changes it.
  useEffect(() => {
    if (!sketchDir) { setBom(null); return; }
    loadBom(sketchDir)
      .then((b) => setBom(b ?? { components: [] }))
      .catch((err) => notify(String(err), true));
  }, [sketchDir, bomVersion]);

  // Reload board info when the project changes.
  useEffect(() => {
    if (!sketchDir) { setBoard(null); return; }
    projectInfo(sketchDir)
      .then((info) => setBoard(boardOf(info.board)))
      .catch(() => setBoard(null));
  }, [sketchDir]);

  // Reload agent-generated SVG when the project or agent writes wiring.svg.
  useEffect(() => {
    if (!sketchDir) { setSvgContent(null); return; }
    readSketchFile(sketchDir, "wiring.svg")
      .then(setSvgContent)
      .catch((err) => {
        const msg = String(err);
        if (msg.includes("not found") || msg.includes("No such file") || msg.includes("os error 2")) {
          setSvgContent(null);
        } else {
          notify(msg, true);
        }
      });
  }, [sketchDir, diagramVersion]);

  const hasWiring = (bom?.components ?? []).some(
    (c) => c.wiring && c.wiring.length > 0,
  );

  // btoa for SVG with unicode characters
  const svgDataUrl = svgContent
    ? `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(svgContent)))}`
    : null;

  return (
    <section
      className="diagram-panel"
      style={active ? undefined : { display: "none" }}
    >
      {!sketchDir ? (
        <div className="diagram-empty">Open a project first.</div>
      ) : !hasWiring && !svgContent ? (
        <div className="diagram-empty">
          <p>No wiring data in <code>bom.yaml</code> yet.</p>
          <p className="diagram-empty-hint">
            Add wiring in the BOM tab, or ask the assistant to fill it in.
          </p>
        </div>
      ) : (
        <div className="diagram-scroll">
          {hasWiring && bom && (
            <div className="diagram-section">
              <span className="diagram-section-title">Wiring diagram</span>
              <div className="diagram-svg-wrap">
                <WiringDiagram bom={bom} board={board} />
              </div>
            </div>
          )}

          {svgDataUrl && (
            <div className="diagram-section">
              <span className="diagram-section-title">
                wiring.svg — generated by assistant
              </span>
              <div className="diagram-svg-wrap">
                <img
                  src={svgDataUrl}
                  alt="Wiring diagram generated by assistant"
                  className="diagram-agent-img"
                />
              </div>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
