import type { SketchFile } from "../api";
import { fileTabs, tabLabel } from "../editorTabs";

interface Props {
  files: SketchFile[];
  openFile: string | null;
  dirtyFiles: Set<string>;
  onOpen: (relPath: string) => void;
}

/** One tab per project file, always visible above the editor — the sidebar's
 *  file tree may be hidden behind another panel, this strip never is. */
export default function EditorTabs({ files, openFile, dirtyFiles, onOpen }: Props) {
  const tabs = fileTabs(files);
  return (
    <div className="panel-tabs editor-tabs">
      {tabs.map((f) => {
        const active = f.rel_path === openFile;
        return (
          <button
            key={f.rel_path}
            className={active ? "tab active" : "tab"}
            title={f.rel_path}
            onClick={() => onOpen(f.rel_path)}
          >
            {tabLabel(f.rel_path)}
            {dirtyFiles.has(f.rel_path) && <span className="tab-dot">●</span>}
          </button>
        );
      })}
      {tabs.length === 0 && <span className="editor-tabs-empty">no file open</span>}
      <div className="spacer" />
      {openFile && dirtyFiles.has(openFile) && (
        <span className="editor-tabs-hint">unsaved (Ctrl+S)</span>
      )}
    </div>
  );
}
