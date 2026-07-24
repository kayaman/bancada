import { useEffect, useRef, useState } from "react";
import type { OutputLine } from "../api";

interface ConsoleProps {
  lines: OutputLine[];
  onClear: () => void;
  /** When set, shows a TX input row (serial monitor mode). */
  onSend?: (data: string) => void;
}

export default function Console({ lines, onClear, onSend }: ConsoleProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  // Autoscroll only while the user is at the bottom, so scrolling up to read
  // an earlier error isn't fought by incoming lines.
  const stickToBottom = useRef(true);
  const [tx, setTx] = useState("");

  useEffect(() => {
    const el = scrollRef.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
  }, [lines]);

  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    stickToBottom.current =
      el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  const send = () => {
    if (onSend && tx) {
      onSend(tx);
      setTx("");
    }
  };

  return (
    <div className="console">
      <div className="console-scroll" ref={scrollRef} onScroll={onScroll}>
        {lines.map((l, i) => (
          <div key={i} className={`console-line ${l.stream}`}>
            {l.line}
          </div>
        ))}
      </div>
      <div className="console-controls">
        {onSend && (
          <input
            className="input mono"
            placeholder="send to board… (Enter)"
            value={tx}
            onChange={(e) => setTx(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && send()}
          />
        )}
        <button className="btn small" onClick={onClear}>
          Clear
        </button>
      </div>
    </div>
  );
}
