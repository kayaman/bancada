import { useExplorerStore } from "../explorerStore";
import Menu from "./Menu";

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
  const protTitle = prot ? `${target} is required by the build` : undefined;

  const item = (
    label: string,
    action: () => void,
    disabled = false,
    title?: string,
  ) => (
    <button
      className="ctx-item"
      role="menuitem"
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
    <Menu x={menu.x} y={menu.y} onClose={close}>
      {item("New File…", () => onNewFile(parentDir))}
      {item("New Folder…", () => onNewFolder(parentDir))}
      {target !== null && onRename && (
        <>
          <div className="ctx-sep" role="separator" />
          {item("Rename…", () => onRename(target), prot, protTitle)}
        </>
      )}
      {target !== null && onDelete && item("Delete", () => onDelete(target), prot, protTitle)}
    </Menu>
  );
}
