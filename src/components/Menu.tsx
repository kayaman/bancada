import { useLayoutEffect, useRef, useState } from "react";

interface Props {
  x: number;
  y: number;
  onClose: () => void;
  /** Anchor button whose mousedown must not count as outside — otherwise a
   *  second click on the anchor closes then immediately reopens. */
  anchorRef?: React.RefObject<HTMLElement | null>;
  /**
   * ARIA role for the popover. `"menu"` — the default — is only valid when
   * every child is a `menuitem`; a popover holding inputs must pass
   * `"group"` instead, or screen readers enter menu navigation and skip the
   * fields entirely. See `GitPill`, whose popover is a small form.
   */
  role?: "menu" | "group";
  /** Accessible name. Required with `role="group"`, which has no implicit one. */
  ariaLabel?: string;
  children: React.ReactNode;
}

/** Generic popover shell: fixed-position at (x, y), clamped to the viewport;
 *  closes on outside press, Escape or window blur. The caller renders the
 *  `.ctx-item` children and decides when the menu exists at all. */
export default function Menu({
  x,
  y,
  onClose,
  anchorRef,
  role = "menu",
  ariaLabel,
  children,
}: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    if (!ref.current) return;
    const r = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(x, window.innerWidth - r.width - 4),
      y: Math.min(y, window.innerHeight - r.height - 4),
    });
  }, [x, y]);

  // Pass a stable onClose — this effect re-subscribes whenever it changes.
  useLayoutEffect(() => {
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (!ref.current?.contains(t) && !anchorRef?.current?.contains(t))
        onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose, anchorRef]);

  return (
    <div
      ref={ref}
      className="ctx-menu"
      role={role}
      aria-label={ariaLabel}
      style={{ left: pos?.x ?? x, top: pos?.y ?? y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {children}
    </div>
  );
}
