import type { BoardOffer as Offer } from "../boardOffer";
import { driftLabel } from "../boardOffer";

interface Props {
  offer: Offer;
  /** Commit count from `git_project_drift`; `null` = cannot compare,
   *  `undefined` = still being fetched. */
  drift: number | null | undefined;
  /** The recorded project directory no longer exists. */
  missing: boolean;
  /**
   * Armed by a first Open click when opening would discard something.
   * The second click commits — the same idiom a dirty tab close uses, and
   * the reason there is no modal here.
   */
  armed: boolean;
  /** Why opening would discard something, shown once armed. */
  armedReason: string | null;
  onOpen: () => void;
  onDismiss: () => void;
}

/**
 * "This board's project is over here" — offered, never taken.
 *
 * A banner rather than a pane: an editor-area pane would join `showPane`'s
 * mutually-exclusive group and be dismissed by the New/Duplicate/Rename forms,
 * and it would cover the editor for something the user did not ask for. A
 * banner rather than the status bar, because `notify` is one string that the
 * next message overwrites — this has to survive until answered.
 *
 * It appears unbidden, possibly mid-keystroke, and its Open button leads to
 * `loadSketch`, which discards unsaved buffers and tears down a live Assistant
 * session without asking. Hence `armed`: the first click explains, the second
 * commits.
 */
export default function BoardOffer(props: Props) {
  const { offer, drift, missing, armed, armedReason } = props;
  const openBlocked = missing ? "that folder is no longer there" : null;

  return (
    <div className="board-offer" role="alert">
      <span className="board-offer-icon" aria-hidden="true">
        ⚡
      </span>
      <span className="board-offer-text">
        <strong>{offer.title}</strong> was last flashed from{" "}
        <span className="board-offer-path">{offer.rec.project_dir}</span>
        {drift !== undefined && !missing && (
          <> — {driftLabel(drift, offer.rec.tag)}</>
        )}
        {missing && <> — that folder is no longer there</>}
        {armed && armedReason && (
          <>
            <br />
            <span className="board-offer-warn">
              {armedReason} — click Open again to continue
            </span>
          </>
        )}
      </span>
      <div className="spacer" />
      <button
        className={`btn small ${armed ? "danger" : "primary"}`}
        onClick={props.onOpen}
        disabled={openBlocked !== null}
        title={openBlocked ?? `Open ${offer.rec.project_dir}`}
      >
        {armed ? "Open anyway" : "Open project"}
      </button>
      <button
        className="btn small icon"
        onClick={props.onDismiss}
        title="Dismiss until this board is plugged in again"
        aria-label="Dismiss"
      >
        ✕
      </button>
    </div>
  );
}
