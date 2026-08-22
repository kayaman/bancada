// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import ToastStack from "../ToastStack";
import type { Toast } from "../../notifications";

afterEach(cleanup);

const noop = () => {};

const toast = (over: Partial<Toast> = {}): Toast => ({
  id: 1,
  kind: "info",
  message: "scanning ports",
  createdAt: 0,
  expiresAt: 3000,
  count: 1,
  ...over,
});

describe("ToastStack", () => {
  it("renders nothing at all when there is nothing to say", () => {
    const { container } = render(<ToastStack toasts={[]} onDismiss={noop} />);
    expect(container.innerHTML).toBe("");
  });

  it("makes an error assertive and everything else polite", () => {
    render(
      <ToastStack
        toasts={[
          toast({ id: 1, kind: "info", message: "scanning ports" }),
          toast({ id: 2, kind: "success", message: "✓ Compiled" }),
          toast({ id: 3, kind: "error", message: "upload failed" }),
        ]}
        onDismiss={noop}
      />,
    );
    expect(screen.getByRole("alert").textContent).toContain("upload failed");
    expect(screen.getAllByRole("status").length).toBe(2);
  });

  it("shows a repeat count only once there is a repeat", () => {
    const { unmount } = render(
      <ToastStack toasts={[toast({ count: 4 })]} onDismiss={noop} />,
    );
    expect(screen.getByText("×4")).toBeTruthy();
    unmount();

    const { container } = render(
      <ToastStack toasts={[toast({ count: 1 })]} onDismiss={noop} />,
    );
    expect(container.querySelector(".toast-count")).toBe(null);
  });

  it("dismisses the toast that was clicked, by id", () => {
    const onDismiss = vi.fn();
    render(
      <ToastStack
        toasts={[toast({ id: 7 }), toast({ id: 9, message: "another" })]}
        onDismiss={onDismiss}
      />,
    );
    const buttons = screen.getAllByRole("button", {
      name: "Dismiss notification",
    });
    buttons[1].click();
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledWith(9);
  });

  it("gives the icon-only close button both a title and a label", () => {
    render(<ToastStack toasts={[toast()]} onDismiss={noop} />);
    const close = screen.getByRole("button", { name: "Dismiss notification" });
    expect(close.getAttribute("title")).toBe("Dismiss");
  });

  it("carries the kind on the card so the accent stripe can colour it", () => {
    const { container } = render(
      <ToastStack
        toasts={[toast({ id: 1, kind: "success", message: "✓ Compiled" })]}
        onDismiss={noop}
      />,
    );
    expect(container.querySelector(".toast-stack")).not.toBe(null);
    expect(container.querySelector(".toast.toast-success")).not.toBe(null);
  });
});
