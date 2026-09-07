// @vitest-environment jsdom
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ResizeHandle from "../ResizeHandle";
afterEach(cleanup);

function Fixture({ orientation }: { orientation: "vertical" | "horizontal" }) {
  const [value, setValue] = useState(280);
  return <ResizeHandle orientation={orientation} label="Resize" className="handle"
    value={value} min={220} max={400} defaultValue={280} onChange={setValue} onPointerDown={vi.fn()} />;
}

it.each(["vertical", "horizontal"] as const)("resizes a %s separator with limits, larger steps, and reset", async (orientation) => {
  const user = userEvent.setup(); render(<Fixture orientation={orientation} />);
  const handle = screen.getByRole("separator");
  await user.tab(); expect(document.activeElement).toBe(handle);
  await user.keyboard(orientation === "vertical" ? "{ArrowRight}" : "{ArrowUp}");
  expect(handle.getAttribute("aria-valuenow")).toBe("290");
  await user.keyboard(orientation === "vertical" ? "{Shift>}{ArrowLeft}{/Shift}" : "{Shift>}{ArrowDown}{/Shift}");
  expect(handle.getAttribute("aria-valuenow")).toBe("240");
  await user.keyboard("{Home}");
  expect(handle.getAttribute("aria-valuenow")).toBe("220");
  await user.keyboard(orientation === "vertical" ? "{ArrowLeft}" : "{ArrowDown}");
  expect(handle.getAttribute("aria-valuenow")).toBe("220");
  await user.keyboard("{End}");
  expect(handle.getAttribute("aria-valuenow")).toBe("400");
  await user.keyboard("{Enter}");
  expect(handle.getAttribute("aria-valuenow")).toBe("280");
});
