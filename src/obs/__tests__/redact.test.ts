import { describe, expect, it } from "vitest";
import { redactPassword } from "../redact";

describe("redactPassword", () => {
  it("redacts mqtt:// with user:pass", () => {
    expect(redactPassword("mqtt://iot:iot@192.168.15.6:1883")).toBe(
      "mqtt://iot:••••@192.168.15.6:1883",
    );
  });

  it("no-op without a password (user only)", () => {
    expect(redactPassword("mqtt://iot@192.168.15.6:1883")).toBe(
      "mqtt://iot@192.168.15.6:1883",
    );
  });

  it("no-op without auth", () => {
    expect(redactPassword("mqtt://192.168.15.6:1883")).toBe(
      "mqtt://192.168.15.6:1883",
    );
    expect(redactPassword("mqtt://broker.local")).toBe("mqtt://broker.local");
  });

  it("redacts ws://", () => {
    expect(redactPassword("ws://user:secret@host:8080")).toBe(
      "ws://user:••••@host:8080",
    );
  });

  it("redacts wss:// and keeps the path", () => {
    expect(redactPassword("wss://user:secret@host:443/stream/live")).toBe(
      "wss://user:••••@host:443/stream/live",
    );
  });

  it("redacts mqtts://", () => {
    expect(redactPassword("mqtts://iot:hunter2@broker:8883")).toBe(
      "mqtts://iot:••••@broker:8883",
    );
  });

  it("redacts weird characters in the password", () => {
    // ':' inside the password: only the FIRST ':' splits user from pass.
    expect(redactPassword("mqtt://u:p:a:s@host")).toBe("mqtt://u:••••@host");
    // '@' inside the password: userinfo ends at the LAST '@' in the authority.
    expect(redactPassword("mqtt://u:p@ss@host:1883")).toBe(
      "mqtt://u:••••@host:1883",
    );
    // percent-encoded and symbol soup
    expect(redactPassword("ws://u:%40%3A!$&'()*+,;=@host")).toBe(
      "ws://u:••••@host",
    );
    // empty password after ':' still counts as a password
    expect(redactPassword("mqtt://u:@host")).toBe("mqtt://u:••••@host");
  });

  it("no-op on unrecognized schemes and non-URLs", () => {
    expect(redactPassword("http://u:p@host")).toBe("http://u:p@host");
    expect(redactPassword("not a url")).toBe("not a url");
    expect(redactPassword("")).toBe("");
  });

  it("does not touch ':'/'@' that appear only after the authority", () => {
    expect(redactPassword("ws://host:8080/a:b@c")).toBe("ws://host:8080/a:b@c");
  });
});
