import { useMemo, useState } from "react";
import { useExplorerStore } from "../explorerStore";
import { buildTree, visibleNodes } from "../fileTreeModel";
import { protectedPaths } from "../explorerOps";
import TreeContextMenu from "./TreeContextMenu";

interface Props {
  sketchDir: string | null;
  openFile: string | null;
  dirtyFiles: Set<string>;
  onOpen: (relPath: string) => void;
  /** Validated create; return true when handled (closes the input row). */
  onCreateEntry: (raw: string, parentDir: string, kind: "file" | "dir") => boolean;
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
}: Props) {
  const files = useExplorerStore((s) => s.files);
  const expanded = useExplorerStore((s) => s.expanded);
  const selected = useExplorerStore((s) => s.selected);
  const creating = useExplorerStore((s) => s.creating);
  const { toggleExpanded, select, openContextMenu, startCreate, cancelCreate, expandTo } =
    useExplorerStore.getState();

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

  return (
    <div
      className="file-tree"
      onContextMenu={(e) => {
        // background only — rows call openContextMenu themselves and stop
        // propagation before this handler sees the event
        e.preventDefault();
        if (sketchDir) openContextMenu(e.clientX, e.clientY, null);
      }}
    >
      {creating?.parentDir === "" && createRow(0)}
      {rows.map(({ node, depth }) => {
        const pad = 8 + depth * 14;
        const isProt = prot.has(node.relPath);
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
                className={`tree-item dir ${selected === node.relPath ? "selected" : ""}`}
                style={{ paddingLeft: pad }}
                title={node.relPath}
                onClick={() => {
                  toggleExpanded(node.relPath);
                  select(node.relPath);
                }}
                onContextMenu={onCtx}
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
      />
    </div>
  );
}
