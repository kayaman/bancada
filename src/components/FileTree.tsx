import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { useExplorerStore } from "../explorerStore";
import { buildTree, visibleNodes } from "../fileTreeModel";
import { createAnchorDir, isDescendant, protectedPaths } from "../explorerOps";

const parentOf = (p: string) => p.split("/").slice(0, -1).join("/");
const basename = (p: string) => p.split("/").pop() ?? p;
import TreeContextMenu from "./TreeContextMenu";

interface Props {
  sketchDir: string | null;
  openFile: string | null;
  dirtyFiles: Set<string>;
  onOpen: (relPath: string) => void;
  /** Validated create; return true when handled (closes the input row). */
  onCreateEntry: (raw: string, parentDir: string, kind: "file" | "dir") => boolean;
  /** Rename-as-move: `to` is the full new rel path. Resolves true on success. */
  onRenameTo: (from: string, to: string) => Promise<boolean>;
  onDelete: (relPath: string) => void;
}

/** Inline rename: the full rel path, with the basename pre-selected so a
 *  plain rename is one keystroke away but path edits (moves) stay possible. */
function RenameRow({
  depth,
  relPath,
  onSubmit,
  onCancel,
}: {
  depth: number;
  relPath: string;
  onSubmit: (to: string) => Promise<boolean>;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(relPath);
  const ref = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    const input = ref.current;
    if (!input) return;
    const start = relPath.lastIndexOf("/") + 1;
    const dot = relPath.lastIndexOf(".");
    input.focus();
    input.setSelectionRange(start, dot > start ? dot : relPath.length);
    // run once for the row's lifetime
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <div className="tree-create" style={{ paddingLeft: 8 + depth * 14 }}>
      <input
        ref={ref}
        className="input tree-create-input"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") void onSubmit(value).then((ok) => ok && onCancel());
          if (e.key === "Escape") onCancel();
        }}
        onBlur={onCancel}
      />
    </div>
  );
}

/** The inline input row for a pending New File / New Folder. */
function CreateRow({
  depth,
  kind,
  onSubmit,
  onCancel,
}: {
  depth: number;
  kind: "file" | "dir";
  onSubmit: (raw: string) => boolean;
  onCancel: () => void;
}) {
  const [value, setValue] = useState("");
  return (
    <div className="tree-create" style={{ paddingLeft: 8 + depth * 14 }}>
      <input
        className="input tree-create-input"
        autoFocus
        placeholder={kind === "dir" ? "new folder" : "new file, e.g. config.h"}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && onSubmit(value)) onCancel();
          if (e.key === "Escape") onCancel();
        }}
        onBlur={onCancel}
      />
    </div>
  );
}

export default function FileTree({
  sketchDir,
  openFile,
  dirtyFiles,
  onOpen,
  onCreateEntry,
  onRenameTo,
  onDelete,
}: Props) {
  const files = useExplorerStore((s) => s.files);
  const expanded = useExplorerStore((s) => s.expanded);
  const selected = useExplorerStore((s) => s.selected);
  const creating = useExplorerStore((s) => s.creating);
  const renaming = useExplorerStore((s) => s.renaming);
  const dragging = useExplorerStore((s) => s.dragging);
  const dropTarget = useExplorerStore((s) => s.dropTarget);
  const {
    toggleExpanded,
    select,
    openContextMenu,
    startCreate,
    cancelCreate,
    startRename,
    cancelRename,
    expandTo,
    setDragging,
    setDropTarget,
  } = useExplorerStore.getState();

  /** A drop is a move into `dir` ("" = root): rejected when nothing is
   *  dragged, the target is the item, its own subtree, or a no-op. */
  const validDrop = (dir: string): boolean =>
    dragging !== null &&
    dir !== dragging &&
    !isDescendant(dir, dragging) &&
    parentOf(dragging) !== dir;

  const dropInto = (dir: string) => {
    if (!dragging || !validDrop(dir)) return;
    const to = dir ? `${dir}/${basename(dragging)}` : basename(dragging);
    void onRenameTo(dragging, to);
    setDragging(null);
  };

  const rows = useMemo(
    () => visibleNodes(buildTree(files), expanded),
    [files, expanded],
  );
  const prot = useMemo(
    () => (sketchDir ? protectedPaths(sketchDir) : new Set<string>()),
    [sketchDir],
  );
  const dirSet = useMemo(
    () => new Set(files.filter((f) => f.is_dir).map((f) => f.rel_path)),
    [files],
  );

  const beginCreate = (parentDir: string, kind: "file" | "dir") => {
    if (parentDir) expandTo(`${parentDir}/x`); // open the parent chain
    startCreate(parentDir, kind);
  };

  const createRow = (depth: number) =>
    creating && (
      <CreateRow
        key="create-row"
        depth={depth}
        kind={creating.kind}
        onSubmit={(raw) => onCreateEntry(raw, creating.parentDir, creating.kind)}
        onCancel={cancelCreate}
      />
    );

  // Header buttons create in the selected folder (or a selected file's
  // parent); the context menu stays for targeted creation.
  const anchor = createAnchorDir(selected, files);

  return (
    <div className="file-tree-panel">
      <div className="tree-header">
        <button
          className="btn icon"
          title={`New file in ${anchor || "the sketch"}…`}
          disabled={!sketchDir}
          onClick={() => beginCreate(anchor, "file")}
        >
          ＋<span className="tree-header-kind">file</span>
        </button>
        <button
          className="btn icon"
          title={`New folder in ${anchor || "the sketch"}…`}
          disabled={!sketchDir}
          onClick={() => beginCreate(anchor, "dir")}
        >
          ＋<span className="tree-header-kind">folder</span>
        </button>
      </div>
    <div
      className={`file-tree ${dropTarget === "" && dragging ? "drop-target" : ""}`}
      onContextMenu={(e) => {
        // background only — rows call openContextMenu themselves and stop
        // propagation before this handler sees the event
        e.preventDefault();
        if (sketchDir) openContextMenu(e.clientX, e.clientY, null);
      }}
      onDragOver={(e) => {
        if (validDrop("")) {
          e.preventDefault();
          setDropTarget("");
        }
      }}
      onDragLeave={() => dropTarget === "" && setDropTarget(null)}
      onDrop={(e) => {
        e.preventDefault();
        dropInto("");
      }}
    >
      {creating?.parentDir === "" && createRow(0)}
      {rows.map(({ node, depth }) => {
        const pad = 8 + depth * 14;
        const isProt = prot.has(node.relPath);
        if (renaming === node.relPath) {
          return (
            <RenameRow
              key={node.relPath}
              depth={depth}
              relPath={node.relPath}
              onSubmit={(to) => onRenameTo(node.relPath, to)}
              onCancel={cancelRename}
            />
          );
        }
        const onCtx = (e: React.MouseEvent) => {
          e.preventDefault();
          e.stopPropagation();
          select(node.relPath);
          openContextMenu(e.clientX, e.clientY, node.relPath);
        };
        if (node.isDir) {
          const open = expanded.has(node.relPath);
          return (
            <div key={node.relPath}>
              <button
                className={`tree-item dir ${selected === node.relPath ? "selected" : ""} ${
                  dropTarget === node.relPath ? "drop-target" : ""
                }`}
                style={{ paddingLeft: pad }}
                title={node.relPath}
                onClick={() => {
                  toggleExpanded(node.relPath);
                  select(node.relPath);
                }}
                onContextMenu={onCtx}
                draggable
                onDragStart={(e) => {
                  setDragging(node.relPath);
                  e.dataTransfer.setData("text/plain", node.relPath);
                  e.dataTransfer.effectAllowed = "move";
                }}
                onDragEnd={() => setDragging(null)}
                onDragOver={(e) => {
                  if (validDrop(node.relPath)) {
                    e.preventDefault();
                    e.stopPropagation();
                    setDropTarget(node.relPath);
                  }
                }}
                onDragLeave={() => dropTarget === node.relPath && setDropTarget(null)}
                onDrop={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  dropInto(node.relPath);
                }}
              >
                <span className="chevron">{open ? "▾" : "▸"}</span>
                {node.name}
              </button>
              {creating?.parentDir === node.relPath && createRow(depth + 1)}
            </div>
          );
        }
        const active = node.relPath === openFile;
        const dirty = dirtyFiles.has(node.relPath);
        return (
          <button
            key={node.relPath}
            className={`tree-item file ${active ? "active" : ""} ${
              selected === node.relPath ? "selected" : ""
            } ${isProt ? "protected" : ""}`}
            style={{ paddingLeft: pad }}
            title={
              isProt
                ? `${node.relPath} — required by the sketch; cannot be renamed, moved or deleted`
                : node.relPath
            }
            onClick={() => {
              select(node.relPath);
              onOpen(node.relPath);
            }}
            onContextMenu={onCtx}
            draggable={!isProt}
            onDragStart={(e) => {
              setDragging(node.relPath);
              e.dataTransfer.setData("text/plain", node.relPath);
              e.dataTransfer.effectAllowed = "move";
            }}
            onDragEnd={() => setDragging(null)}
          >
            <span className="chevron spacer" />
            {node.name}
            {dirty ? " ●" : ""}
          </button>
        );
      })}
      {files.length === 0 && (
        <div className="empty-hint">Open a sketch folder to begin.</div>
      )}
      <TreeContextMenu
        isDir={(p) => dirSet.has(p)}
        isProtected={(p) => prot.has(p)}
        onNewFile={(parent) => beginCreate(parent, "file")}
        onNewFolder={(parent) => beginCreate(parent, "dir")}
        onRename={startRename}
        onDelete={onDelete}
      />
    </div>
    </div>
  );
}
