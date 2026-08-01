import { useState } from "react";

interface Props {
  title: string;
  /** Return true when handled — the input closes; false keeps it open. */
  onSubmit: (raw: string) => boolean;
}

/** A ＋ that turns into a filename input in place. Enter submits, Escape or
 *  clicking away cancels. */
export default function NewFileInput({ title, onSubmit }: Props) {
  const [open, setOpen] = useState(false);
  const [value, setValue] = useState("");
  const close = () => {
    setOpen(false);
    setValue("");
  };
  if (!open) {
    return (
      <button className="btn icon" title={title} onClick={() => setOpen(true)}>
        ＋
      </button>
    );
  }
  return (
    <input
      className="input new-file-input"
      autoFocus
      placeholder="new file, e.g. config.h"
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && onSubmit(value)) close();
        if (e.key === "Escape") close();
      }}
      onBlur={close}
    />
  );
}
