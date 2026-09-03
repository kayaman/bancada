import { useCallback, useEffect, useState } from "react";
import {
  boardCatalog,
  boardOf,
  projectInfo,
  setProjectBoard,
  type Board,
  type BoardChoice,
  type Caveat,
  type CaveatInfo,
} from "../api";

interface Props {
  sketchDir: string | null;
  notify: (msg: string, isError?: boolean) => void;
}

/**
 * The board reference: which devkit the project is on, and what each pin
 * carries.
 *
 * Reference material, not a control surface. The one thing it *writes* is the
 * board record itself — which is what turns an inferred board into a recorded
 * one, the only transition that exists, since resolving a board never writes.
 *
 * The recorded/inferred distinction is rendered rather than flattened. Both
 * produce the same pin table; only one is a fact the project states about
 * itself. Collapsing them would put a guess and a certainty behind identical
 * pixels, which is the failure the whole board model is built to avoid.
 */
export default function BoardPinout({ sketchDir, notify }: Props) {
  const [choice, setChoice] = useState<BoardChoice | null>(null);
  const [catalog, setCatalog] = useState<Board[]>([]);
  const [glossary, setGlossary] = useState<CaveatInfo[]>([]);
  const [working, setWorking] = useState(false);

  const reload = useCallback(() => {
    if (!sketchDir) {
      setChoice(null);
      return;
    }
    projectInfo(sketchDir)
      .then((i) => setChoice(i.board))
      .catch((e) => notify(String(e), true));
  }, [sketchDir, notify]);

  useEffect(reload, [reload]);

  useEffect(() => {
    boardCatalog()
      .then((c) => {
        setCatalog(c.boards);
        setGlossary(c.caveats);
      })
      .catch(() => {
        // The pin table still renders from the project's own board; only the
        // "record a different board" list and the badge tooltips go missing.
      });
  }, []);

  const record = async (id: string) => {
    if (!sketchDir || !id) return;
    setWorking(true);
    try {
      await setProjectBoard(sketchDir, id);
      reload();
    } catch (e) {
      notify(String(e), true);
    } finally {
      setWorking(false);
    }
  };

  if (!sketchDir) {
    return <div className="empty-hint">Open a project to see its board.</div>;
  }
  if (!choice) return <div className="empty-hint">Reading the project…</div>;

  const board = boardOf(choice);
  const advice = (c: Caveat) => glossary.find((g) => g.id === c);

  return (
    <div className="board-pinout">
      {choice.state === "no-profile" && (
        <div className="empty-hint">
          Bancada carries no pinout for this project's chip. That means unknown,
          not safe — check the board's own documentation before wiring.
        </div>
      )}

      {choice.state === "unchosen" && (
        <div className="np-note">
          Several boards use this chip and the project does not say which.
          Pick one to record it:
          <ul>
            {choice.candidates.map((b) => (
              <li key={b.id}>
                <button
                  className="btn small"
                  disabled={working}
                  onClick={() => record(b.id)}
                >
                  {b.name}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {board && (
        <>
          <div className="bp-head">
            <strong>{board.name}</strong>
            <span className="lib-version">{board.target}</span>
            {choice.state === "inferred" ? (
              <span
                className="bp-inferred"
                title="No board is recorded for this project — this is the only board Bancada models for its chip."
              >
                inferred
              </span>
            ) : (
              <span className="scope-dim">recorded in the project</span>
            )}
          </div>

          {choice.state === "inferred" && (
            <div className="np-note">
              Nothing in this project names a board. The pinout below is the
              only one Bancada models for this chip, so it is very likely
              right — but it is a guess until you record it.
              <button
                className="btn small primary"
                disabled={working}
                onClick={() => record(board.id)}
              >
                Record {board.name}
              </button>
            </div>
          )}

          {board.led && (
            <div className="scope-dim">
              onboard LED on GPIO{board.led.gpio}
              {board.led.kind === "ws2812"
                ? " — addressable WS2812, a plain HIGH/LOW does nothing to it"
                : ""}
            </div>
          )}

          {board.headers.map((h) => (
            <div key={h.name} className="bp-header">
              <div className="bp-header-name">{h.name}</div>
              <table className="bp-pins">
                <tbody>
                  {h.pins.map((p, i) => (
                    <tr key={`${h.name}-${i}`}>
                      <td className="bp-label">{p.label}</td>
                      <td className="bp-gpio">
                        {p.gpio === null ? "" : `GPIO${p.gpio}`}
                      </td>
                      <td className="bp-fns">{p.functions.join(" · ")}</td>
                      <td className="bp-caveats">
                        {p.caveats.map((c) => (
                          <span
                            key={c}
                            className="bp-caveat"
                            title={advice(c)?.advice ?? c}
                          >
                            {advice(c)?.label ?? c}
                          </span>
                        ))}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ))}

          {catalog.length > 1 && (
            <label className="field">
              Not this board?
              <select
                className="select"
                value={board.id}
                disabled={working}
                onChange={(e) => record(e.target.value)}
              >
                {catalog.map((b) => (
                  <option key={b.id} value={b.id}>
                    {b.name}
                  </option>
                ))}
              </select>
            </label>
          )}

          {board.sources.length > 0 && (
            <div className="scope-dim">
              transcribed from {board.sources.join(", ")}
            </div>
          )}
        </>
      )}
    </div>
  );
}
