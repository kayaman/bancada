import { describe, expect, it } from "vitest";
import { exchangeRow } from "../components/DeviceBrowserPanel";

describe("exchangeRow", () => {
  it("labels an exchange with method, path, status, timing and size", () => {
    const r = exchangeRow({
      type: "exchange",
      method: "GET",
      path: "/data.json",
      status: 200,
      duration_ms: 12,
      content_type: "application/json",
      req_bytes: 0,
      resp_bytes: 42,
      preview: '{"t":24.5}',
      truncated: false,
      binary: false,
    });
    expect(r.topic).toBe("GET /data.json → 200 (12 ms, 42 B)");
    expect(r.payload).toBe('{"t":24.5}');
  });

  it("marks truncated previews and scales sizes to KiB", () => {
    const r = exchangeRow({
      type: "exchange",
      method: "GET",
      path: "/big",
      status: 200,
      duration_ms: 3,
      content_type: "text/html",
      req_bytes: 0,
      resp_bytes: 4096,
      preview: "<html>",
      truncated: true,
      binary: false,
    });
    expect(r.topic).toContain("4.0 KiB");
    expect(r.payload).toBe("<html>…");
  });

  it("flags binary bodies and describes empty ones honestly", () => {
    const bin = exchangeRow({
      type: "exchange", method: "GET", path: "/img.png", status: 200,
      duration_ms: 5, content_type: "image/png", req_bytes: 0, resp_bytes: 900,
      preview: "89 50 4e 47", truncated: true, binary: true,
    });
    expect(bin.payload).toBe("(binary 900 B) 89 50 4e 47");
    const empty = exchangeRow({
      type: "exchange", method: "POST", path: "/led", status: 204,
      duration_ms: 2, content_type: null, req_bytes: 4, resp_bytes: 0,
      preview: "", truncated: false, binary: false,
    });
    expect(empty.payload).toBe("(no content-type, empty body)");
  });
});
