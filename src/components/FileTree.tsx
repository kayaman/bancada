import type { SketchFile } from "../api";

interface Props {
  files: SketchFile[];
  openFile: string | null;
  dirtyFiles: Set<string>;
  onOpen: (relPath: string) => void;
}

export default function FileTree({ files, openFile, dirtyFiles, onOpen }: Props) {
  return (
    <div className="file-tree">
      {files.map((f) => {
        const depth = f.rel_path.split("/").length - 1;
        const name = f.rel_path.split("/").pop();
        if (f.is_dir) {
          return (
            <div
              key={f.rel_path}
              className="tree-item dir"
              style={{ paddingLeft: 8 + depth * 14 }}
            >
              {name}/
            </div>
          );
        }
        const active = f.rel_path === openFile;
        const dirty = dirtyFiles.has(f.rel_path);
        return (
          <button
            key={f.rel_path}
            className={`tree-item file ${active ? "active" : ""}`}
            style={{ paddingLeft: 8 + depth * 14 }}
            onClick={() => onOpen(f.rel_path)}
            title={f.rel_path}
          >
            {name}
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
