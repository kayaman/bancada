// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import TargetPicker from "../TargetPicker";

afterEach(cleanup);

const TARGETS = ["esp32", "esp32c3", "esp32c6", "esp32s3"];

function setup(over: Partial<React.ComponentProps<typeof TargetPicker>> = {}) {
  const onSetTarget = vi.fn();
  render(
    <TargetPicker
      targets={TARGETS}
      current="esp32s3"
      sketchDir="/p"
      busy={false}
      idfAvailable
      underGit
      hasSdkconfig
      onSetTarget={onSetTarget}
      {...over}
    />,
  );
  return { onSetTarget };
}

const select = () => screen.getByLabelText("ESP-IDF target") as HTMLSelectElement;

describe("TargetPicker", () => {
  it("does not apply a target just because the select changed", () => {
    // set-target deletes build/ and regenerates sdkconfig. Choosing must arm
    // a confirmation, never fire the destructive call on its own.
    const { onSetTarget } = setup();
    fireEvent.change(select(), { target: { value: "esp32c6" } });
    expect(onSetTarget).not.toHaveBeenCalled();
    expect(screen.getByText("Set esp32c6")).toBeTruthy();
  });

  it("applies only when the confirmation is pressed", () => {
    const { onSetTarget } = setup();
    fireEvent.change(select(), { target: { value: "esp32c6" } });
    fireEvent.click(screen.getByText("Set esp32c6"));
    expect(onSetTarget).toHaveBeenCalledWith("esp32c6");
  });

  it("cancel restores the shown value to the real target", () => {
    // A select left displaying a target that was never applied is a lie the
    // user would go on to act on.
    const { onSetTarget } = setup();
    fireEvent.change(select(), { target: { value: "esp32c6" } });
    expect(select().value).toBe("esp32c6");
    fireEvent.click(screen.getByLabelText("Cancel target change"));
    expect(select().value).toBe("esp32s3");
    expect(onSetTarget).not.toHaveBeenCalled();
  });

  it("says there is no undo when the project is not under git", () => {
    setup({ underGit: false });
    fireEvent.change(select(), { target: { value: "esp32c6" } });
    expect(screen.getByText("⚠ not under git")).toBeTruthy();
  });

  it("skips the confirmation when there is nothing to lose", () => {
    // A fresh project with no sdkconfig has no hand-edited configuration to
    // destroy. Confirming a no-op is how confirmations become noise.
    const { onSetTarget } = setup({ current: null, hasSdkconfig: false });
    fireEvent.change(select(), { target: { value: "esp32c3" } });
    expect(onSetTarget).toHaveBeenCalledWith("esp32c3");
  });

  it("is disabled with a reason while a build is running", () => {
    setup({ busy: true });
    expect(select().disabled).toBe(true);
    expect(select().title).toBe("a build is already running");
  });

  it("is disabled with a reason when ESP-IDF is unavailable", () => {
    setup({ idfAvailable: false });
    expect(select().title).toBe("ESP-IDF is not available on this machine");
  });
});
