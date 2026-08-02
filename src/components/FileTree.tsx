import { useMemo } from "react";
import { useExplorerStore } from "../explorerStore";
import { buildTree, visibleNodes } from "../fileTreeModel";
import { protectedPaths } from "../explorerOps";
import NewFileInput from "./NewFileInput";

interface Props {
  sketchDir: string | null;
  openFile: string | null;
  dirtyFiles: Set<string>;
  onOpen: (relPath: string) => void;
  /** Raw name from the ＋ input; return true when handled. */
  onCreate?: (raw: string) => boolean;
}

export default function FileTree({
  sketchDir,
  openFile,
  dirtyFiles,
  onOpen,
  onCreate,
}: Props) {
  const files = useExplorerStore((s) => s.files);
  const expanded = useExplorerStore((s) => s.expanded);
  const selected = useExplorerStore((s) => s.selected);
  const { toggleExpanded, select } = useExplorerStore.getState();

  const rows = useMemo(
    () => visibleNodes(buildTree(files), expanded),
    [files, expanded],
  );
  const prot = useMemo(
    () => (sketchDir ? protectedPaths(sketchDir) : new Set<string>()),
    [sketchDir],
  );

  return (
    <div className="file-tree">
      {onCreate && (
        <div className="tree-new">
          <NewFileInput title="New file in this sketch" onSubmit={onCreate} />
        </div>
      )}
      {rows.map(({ node, depth }) => {
        const pad = 8 + depth * 14;
        if (node.isDir) {
          const open = expanded.has(node.relPath);
          return (
            <button
              key={node.relPath}
              className={`tree-item dir ${selected === node.relPath ? "selected" : ""}`}
              style={{ paddingLeft: pad }}
              title={node.relPath}
              onClick={() => {
                toggleExpanded(node.relPath);
                select(node.relPath);
              }}
            >
              <span className="chevron">{open ? "▾" : "▸"}</span>
              {node.name}
            </button>
          );
        }
        const isProt = prot.has(node.relPath);
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
    </div>
  );
}
