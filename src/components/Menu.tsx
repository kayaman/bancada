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

  const items = () => Array.from(ref.current?.querySelectorAll<HTMLElement>(
    '[role="menuitem"]:not(:disabled):not([aria-disabled="true"])',
  ) ?? []).filter((item) => item.closest('[role="menu"]') === ref.current);

  useLayoutEffect(() => {
    const menu = ref.current;
    const previous = anchorRef?.current ?? document.activeElement as HTMLElement | null;
    if (role === "menu") {
      items().forEach((item) => { item.tabIndex = -1; });
      (items()[0] ?? menu)?.focus();
    } else {
      menu?.querySelector<HTMLElement>('input:not(:disabled), button:not(:disabled), select:not(:disabled)')?.focus();
    }
    return () => {
      // A pointer click elsewhere owns focus; dismissal must not steal it.
      if (menu?.contains(document.activeElement) || document.activeElement === document.body) {
        if (previous?.isConnected) previous.focus();
      }
    };
  }, []);

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
      tabIndex={-1}
      aria-label={ariaLabel}
      style={{ left: pos?.x ?? x, top: pos?.y ?? y }}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        if (e.key === "Escape" || (e.key === "ArrowLeft" && role === "menu" && anchorRef?.current?.closest('[role="menu"]'))) {
          e.preventDefault();
          e.stopPropagation();
          onClose();
          return;
        }
        if (role !== "menu") return;
        if (e.key === "Tab") {
          // Let nested Tab bubble to the outer menu, then tab from its trigger.
          if (ref.current?.parentElement?.closest('[role="menu"]')) return;
          anchorRef?.current?.focus();
          onClose();
          return;
        }
        const enabled = items();
        const index = enabled.indexOf(document.activeElement as HTMLElement);
        let next: number;
        if (e.key === "ArrowDown") next = (index + 1) % enabled.length;
        else if (e.key === "ArrowUp") next = (index - 1 + enabled.length) % enabled.length;
        else if (e.key === "Home") next = 0;
        else if (e.key === "End") next = enabled.length - 1;
        else if (e.key === "ArrowRight" && enabled[index]?.getAttribute("aria-haspopup") === "menu") {
          e.preventDefault();
          e.stopPropagation();
          if (enabled[index].getAttribute("aria-expanded") !== "true") enabled[index].click();
          return;
        } else return;
        e.preventDefault();
        e.stopPropagation();
        enabled[next]?.focus();
      }}
    >
      {children}
    </div>
  );
}
