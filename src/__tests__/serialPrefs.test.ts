import { describe, expect, it } from "vitest";
import {
  BAUD_KEY,
  BAUD_RATES,
  DEFAULT_BAUD,
  DEFAULT_UI_PREFS,
  LINE_ENDINGS,
  LINE_ENDING_LABEL,
  UI_KEY,
  detectBaud,
  effectiveBaud,
  lineEndingBytes,
  loadBaudOverrides,
  loadUiPrefs,
  saveBaudOverride,
  saveUiPrefs,
  withLineEnding,
  type StorageLike,
} from "../serialPrefs";

/** A `localStorage` stand-in: same three methods, none of the browser. */
function fakeStorage(seed: Record<string, string> = {}): StorageLike & {
  map: Map<string, string>;
} {
  const map = new Map(Object.entries(seed));
  return {
    map,
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
    removeItem: (k) => void map.delete(k),
  };
}

describe("baud list", () => {
  it("offers 74880 — the ESP8266 boot-ROM rate the old fixed list lacked", () => {
    expect(BAUD_RATES).toContain(74880);
    expect(DEFAULT_BAUD).toBe(115200);
  });

  it("is sorted ascending with no duplicates", () => {
    expect([...BAUD_RATES]).toEqual([...new Set(BAUD_RATES)].sort((a, b) => a - b));
  });
});

describe("lineEndingBytes / withLineEnding", () => {
  it("maps every ending to its bytes", () => {
    expect(lineEndingBytes("none")).toBe("");
    expect(lineEndingBytes("nl")).toBe("\n");
    expect(lineEndingBytes("cr")).toBe("\r");
    // Arduino IDE's "Both NL & CR" sends CR then LF.
    expect(lineEndingBytes("nlcr")).toBe("\r\n");
  });

  it("labels and byte maps cover the whole union", () => {
    for (const e of LINE_ENDINGS) {
      expect(LINE_ENDING_LABEL[e]).toBeTruthy();
      expect(typeof lineEndingBytes(e)).toBe("string");
    }
    expect(LINE_ENDINGS).toHaveLength(4);
  });

  it("appends the ending to the data", () => {
    expect(withLineEnding("AT", "nlcr")).toBe("AT\r\n");
    expect(withLineEnding("AT", "none")).toBe("AT");
    expect(withLineEnding("", "nl")).toBe("\n");
  });
});

describe("detectBaud", () => {
  it("finds a plain literal", () => {
    expect(detectBaud(["Serial.begin(115200);"])).toBe(115200);
  });

  it("ignores a second argument", () => {
    expect(detectBaud(["Serial.begin(9600, SERIAL_8N1);"])).toBe(9600);
  });

  it("tolerates a UL suffix", () => {
    expect(detectBaud(["Serial.begin(74880UL);"])).toBe(74880);
  });

  it("tolerates digit separators", () => {
    expect(detectBaud(["Serial.begin(1'000'000);"])).toBe(1000000);
  });

  it("accepts two files that agree", () => {
    expect(
      detectBaud([
        "void setup(){Serial.begin(230400);}",
        "void reopen(){Serial.begin(230400);}",
      ]),
    ).toBe(230400);
  });

  it("refuses two files that disagree", () => {
    expect(
      detectBaud(["Serial.begin(9600);", "Serial.begin(115200);"]),
    ).toBeNull();
  });

  it("skips a line comment and keeps the real call", () => {
    expect(detectBaud(["// Serial.begin(9600);\nSerial.begin(115200);"])).toBe(
      115200,
    );
  });

  it("skips a block comment", () => {
    expect(
      detectBaud(["/* Serial.begin(9600);\n   more */\nSerial.begin(57600);"]),
    ).toBe(57600);
  });

  it("does not mistake Serial1 for Serial", () => {
    expect(detectBaud(["Serial1.begin(9600);"])).toBeNull();
    expect(detectBaud(["Serial2.begin(9600);"])).toBeNull();
  });

  it("resolves a #define one level", () => {
    expect(
      detectBaud(["#define BAUD 250000\nvoid setup(){Serial.begin(BAUD);}"]),
    ).toBe(250000);
  });

  it("resolves a const one level, across files", () => {
    expect(
      detectBaud([
        "const unsigned long BAUD = 500000;",
        "void setup(){Serial.begin(BAUD);}",
      ]),
    ).toBe(500000);
  });

  it("resolves a constexpr", () => {
    expect(
      detectBaud(["constexpr uint32_t BAUD = 921600;\nSerial.begin(BAUD);"]),
    ).toBe(921600);
  });

  it("resolves a parenthesised define", () => {
    expect(detectBaud(["#define BAUD (115200)\nSerial.begin(BAUD);"])).toBe(
      115200,
    );
  });

  it("gives up on an identifier it cannot resolve", () => {
    expect(detectBaud(["Serial.begin(MYSTERY_BAUD);"])).toBeNull();
  });

  it("returns null for no sources and for no call", () => {
    expect(detectBaud([])).toBeNull();
    expect(detectBaud(["void loop() {}"])).toBeNull();
  });

  it("reports a rate that is not in the offered list", () => {
    // The sketch is the authority; the picker widens to fit it.
    expect(detectBaud(["Serial.begin(38400);"])).toBe(38400);
  });
});

describe("baud overrides in storage", () => {
  it("round-trips a per-sketch override", () => {
    const s = fakeStorage();
    const map = saveBaudOverride(s, "/p/Blink", 74880);
    expect(map).toEqual({ "/p/Blink": 74880 });
    expect(loadBaudOverrides(s)).toEqual({ "/p/Blink": 74880 });
    expect(s.map.get(BAUD_KEY)).toBeTruthy();
  });

  it("null removes only that sketch's key", () => {
    let s = fakeStorage();
    saveBaudOverride(s, "/a", 9600);
    saveBaudOverride(s, "/b", 57600);
    const map = saveBaudOverride(s, "/a", null);
    expect(map).toEqual({ "/b": 57600 });
    expect(loadBaudOverrides(s)).toEqual({ "/b": 57600 });
  });

  it("treats garbage as an empty map", () => {
    expect(loadBaudOverrides(fakeStorage({ [BAUD_KEY]: "{not json" }))).toEqual({});
    expect(loadBaudOverrides(fakeStorage({ [BAUD_KEY]: "[1,2]" }))).toEqual({});
    expect(
      loadBaudOverrides(fakeStorage({ [BAUD_KEY]: '{"/a":"fast"}' })),
    ).toEqual({});
  });
});

describe("ui prefs in storage", () => {
  it("defaults when nothing is stored", () => {
    expect(loadUiPrefs(fakeStorage())).toEqual(DEFAULT_UI_PREFS);
  });

  it("round-trips", () => {
    const s = fakeStorage();
    const prefs = { lineEnding: "cr" as const, timestamps: true, autoscroll: false };
    saveUiPrefs(s, prefs);
    expect(s.map.get(UI_KEY)).toBeTruthy();
    expect(loadUiPrefs(s)).toEqual(prefs);
  });

  it("falls back to defaults on garbage and on an unknown line ending", () => {
    expect(loadUiPrefs(fakeStorage({ [UI_KEY]: "nope" }))).toEqual(DEFAULT_UI_PREFS);
    expect(
      loadUiPrefs(fakeStorage({ [UI_KEY]: '{"lineEnding":"lf","timestamps":1}' })),
    ).toEqual({ lineEnding: "nl", timestamps: true, autoscroll: true });
  });
});

describe("effectiveBaud", () => {
  it("falls back to the default", () => {
    expect(effectiveBaud(undefined, null)).toEqual({
      baud: 115200,
      source: "default",
    });
  });

  it("prefers the sketch over the default", () => {
    expect(effectiveBaud(undefined, 74880)).toEqual({
      baud: 74880,
      source: "sketch",
    });
  });

  it("prefers the user's override over the sketch", () => {
    expect(effectiveBaud(9600, 74880)).toEqual({ baud: 9600, source: "override" });
  });
});
