// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

afterEach(cleanup);

describe("component harness", () => {
  it("renders and queries by role", () => {
    render(<button aria-label="x">x</button>);
    expect(screen.getByRole("button", { name: "x" })).toBeTruthy();
  });
});
