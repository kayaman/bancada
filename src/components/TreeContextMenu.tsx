import { useLayoutEffect, useRef, useState } from "react";
import { useExplorerStore } from "../explorerStore";

interface Props {
  /** Is the target rel_path a directory? (root target is null) */
  isDir: (relPath: string) => boolean;
  isProtected: (relPath: string) => boolean;
  onNewFile: (parentDir: string) => void;
  onNewFolder: (parentDir: string) => void;
  onRename?: (relPath: string) => void;
  onDelete?: (relPath: string) => void;
}

/** Hand-rolled context menu for the file tree. Fixed-position at the click
 *  point, clamped to the viewport; closes on outside press, Escape, blur or
 *  item click. Rendered only while the store has contextMenu state. */
export default function TreeContextMenu({
  isDir,
  isProtected,
  onNewFile,
  onNewFolder,
  onRename,
  onDelete,
}: Props) {
  const menu = useExplorerStore((s) => s.contextMenu);
  const close = useExplorerStore((s) => s.closeContextMenu);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);

  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    setPos({
      x: Math.min(menu.x, window.innerWidth - r.width - 4),
      y: Math.min(menu.y, window.innerHeight - r.height - 4),
    });
  }, [menu]);

  useLayoutEffect(() => {
    if (!menu) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("blur", close);
    };
  }, [menu, close]);

  if (!menu) return null;
  const target = menu.target;
  const dirTarget = target !== null && isDir(target);
  // "New …" creates inside a dir target, or beside a file target's parent.
  const parentDir =
    target === null
      ? ""
      : dirTarget
        ? target
        : target.split("/").slice(0, -1).join("/");
  const prot = target !== null && isProtected(target);
  const protTitle = prot ? `${target} is required by the sketch` : undefined;

  const item = (
    label: string,
    action: () => void,
    disabled = false,
    title?: string,
  ) => (
    <button
      className="ctx-item"
      disabled={disabled}
      title={title}
      onClick={() => {
        close();
        action();
      }}
    >
      {label}
    </button>
  );

  return (
    <div
      ref={ref}
      className="ctx-menu"
      style={{ left: pos?.x ?? menu.x, top: pos?.y ?? menu.y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {item("New File…", () => onNewFile(parentDir))}
      {item("New Folder…", () => onNewFolder(parentDir))}
      {target !== null && onRename && (
        <>
          <div className="ctx-sep" />
          {item("Rename…", () => onRename(target), prot, protTitle)}
        </>
      )}
      {target !== null && onDelete && item("Delete", () => onDelete(target), prot, protTitle)}
    </div>
  );
}
