import type { CircuitValidation } from "../api";

interface Props {
  validation: CircuitValidation | null;
  projectOpen: boolean;
  onOpen: () => void;
}

export default function CircuitStatusPanel({ validation, projectOpen, onOpen }: Props) {
  const errors = validation?.diagnostics.filter((item) => item.severity === "error").length ?? 0;
  const warnings = validation?.diagnostics.filter((item) => item.severity === "warning").length ?? 0;
  return (
    <div className="circuit-side-status">
      <span className={`circuit-badge ${validation?.valid ? "valid" : validation ? "blocked" : "neutral"}`}>
        {!projectOpen ? "No project" : !validation ? "No circuit" : validation.valid ? "Build ready" : "Build blocked"}
      </span>
      {validation && <p>{errors} errors · {warnings} warnings</p>}
      <p className="circuit-hint">One versioned hardware manifest stays synchronized with Arduino C++ pin definitions and generated wiring docs.</p>
      <button className="primary" disabled={!projectOpen} onClick={onOpen}>{validation ? "Open circuit" : "Create circuit"}</button>
    </div>
  );
}
