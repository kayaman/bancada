// @vitest-environment jsdom
import { useRef, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import Menu from "../Menu";
import ProjectMenu from "../ProjectMenu";

vi.mock("../../api", () => ({ loadSettings: vi.fn(async () => ({ recent_projects: ["/tmp/one", "/tmp/two"] })) }));
afterEach(cleanup);

function Fixture() {
  const anchor = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  return <>
    <button ref={anchor} onClick={() => setOpen(true)}>Open</button>
    {open && <Menu x={0} y={0} anchorRef={anchor} onClose={() => setOpen(false)}>
      <button role="menuitem">First</button>
      <button role="menuitem" disabled>Disabled</button>
      <button role="menuitem">Last</button>
    </Menu>}
    <button>Outside</button>
  </>;
}

describe("menu keyboard navigation", () => {
  it("focuses the first item, skips disabled items, wraps, and supports Home/End", async () => {
    const user = userEvent.setup(); render(<Fixture />);
    await user.click(screen.getByText("Open"));
    expect(document.activeElement).toBe(screen.getByText("First"));
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByText("Last"));
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(screen.getByText("First"));
    await user.keyboard("{ArrowUp}");
    expect(document.activeElement).toBe(screen.getByText("Last"));
    await user.keyboard("{Home}");
    expect(document.activeElement).toBe(screen.getByText("First"));
    await user.keyboard("{End}{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(screen.getByText("Open"));
  });

  it("Tab leaves the menu and outside clicks retain focus", async () => {
    const user = userEvent.setup(); render(<Fixture />);
    await user.click(screen.getByText("Open"));
    await user.tab();
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(screen.getByText("Outside"));
    await user.click(screen.getByText("Open"));
    await user.click(screen.getByText("Outside"));
    expect(document.activeElement).toBe(screen.getByText("Outside"));
  });

  it("navigates Recent with arrows, closes one level at a time, and activates with Enter", async () => {
    const user = userEvent.setup(); const onOpenRecent = vi.fn();
    render(<ProjectMenu sketchDir="/tmp/project" onOpen={vi.fn()} onNew={vi.fn()} onDuplicate={vi.fn()} onRename={vi.fn()} onOpenRecent={onOpenRecent} />);
    const trigger = screen.getByRole("button", { name: /project/i });
    trigger.focus();
    await user.keyboard("{ArrowDown}{ArrowDown}{ArrowRight}");
    expect(document.activeElement).toBe(await screen.findByRole("menuitem", { name: "one" }));
    await user.keyboard("{ArrowLeft}");
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: /Recent/ }));
    expect(screen.getAllByRole("menu")).toHaveLength(1);
    await user.keyboard("{ArrowRight}{Escape}");
    expect(screen.getAllByRole("menu")).toHaveLength(1);
    await user.keyboard("{ArrowRight}{ArrowDown}{Enter}");
    expect(onOpenRecent).toHaveBeenCalledWith("/tmp/two");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
});
