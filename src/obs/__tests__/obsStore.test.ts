import { describe, expect, it } from "vitest";
import { ObsMsg, ObsStore } from "../obsStore";

const T0 = 1_753_400_000_000;

function msg(over: Partial<ObsMsg> = {}): ObsMsg {
  return {
    topic: "bancada/test",
    payload: "hi",
    b64: false,
    retain: false,
    qos: 0,
    ts: T0,
    ...over,
  };
}

describe("ObsStore ring", () => {
  it("appends oldest→newest and evicts past cap", () => {
    const s = new ObsStore(3);
    for (let i = 0; i < 5; i++) {
      s.push(msg({ payload: `m${i}`, ts: T0 + i }));
    }
    const snap = s.snapshot();
    expect(snap.msgs.map((m) => m.payload)).toEqual(["m2", "m3", "m4"]);
  });

  it("defaults to a 500-message cap", () => {
    const s = new ObsStore();
    for (let i = 0; i < 510; i++) s.push(msg({ payload: `m${i}`, ts: T0 + i }));
    const snap = s.snapshot();
    expect(snap.msgs.length).toBe(500);
    expect(snap.msgs[0].payload).toBe("m10");
    expect(snap.msgs[499].payload).toBe("m509");
  });

  it("clear drops messages, topics and buffered count", () => {
    const s = new ObsStore(10);
    s.push(msg());
    s.setPaused(true);
    s.push(msg());
    s.clear();
    const snap = s.snapshot();
    expect(snap.msgs).toEqual([]);
    expect(snap.topics).toEqual([]);
    expect(snap.bufferedWhilePaused).toBe(0);
    expect(snap.paused).toBe(true); // pause state survives clear
  });
});

describe("ObsStore pause", () => {
  it("buffers counts while paused without appending to the ring", () => {
    const s = new ObsStore(10);
    s.push(msg({ payload: "before", ts: T0 }));
    s.setPaused(true);
    s.push(msg({ payload: "dropped1", ts: T0 + 1 }));
    s.push(msg({ payload: "dropped2", ts: T0 + 2 }));
    const snap = s.snapshot();
    expect(snap.paused).toBe(true);
    expect(snap.bufferedWhilePaused).toBe(2);
    expect(snap.msgs.map((m) => m.payload)).toEqual(["before"]);
  });

  it("topic stats still update while paused", () => {
    const s = new ObsStore(10);
    s.setPaused(true);
    s.push(msg({ topic: "a", payload: "x", ts: T0 }));
    s.push(msg({ topic: "a", payload: "y", ts: T0 + 1 }));
    const [[name, stat]] = s.snapshot().topics;
    expect(name).toBe("a");
    expect(stat.count).toBe(2);
    expect(stat.last).toBe("y");
    expect(stat.lastTs).toBe(T0 + 1);
  });

  it("unpausing clears bufferedWhilePaused and buffered msgs stay dropped", () => {
    const s = new ObsStore(10);
    s.setPaused(true);
    s.push(msg({ payload: "gone", ts: T0 }));
    s.setPaused(false);
    const snap = s.snapshot();
    expect(snap.paused).toBe(false);
    expect(snap.bufferedWhilePaused).toBe(0);
    expect(snap.msgs).toEqual([]); // NOT retained
    // and new messages flow again
    s.push(msg({ payload: "live", ts: T0 + 1 }));
    expect(s.snapshot().msgs.map((m) => m.payload)).toEqual(["live"]);
  });
});

describe("ObsStore rate window (injected clock)", () => {
  it("ratePerSec = messages in trailing 10s / 10", () => {
    const s = new ObsStore(100);
    for (let i = 0; i < 5; i++) s.push(msg({ topic: "t", ts: T0 + i * 1000 }));
    const [[, stat]] = s.snapshot().topics;
    expect(stat.ratePerSec).toBe(0.5); // 5 msgs / 10 s
  });

  it("tick(now) prunes timestamps that fell out of the window", () => {
    const s = new ObsStore(100);
    s.push(msg({ topic: "t", ts: T0 }));
    s.push(msg({ topic: "t", ts: T0 + 9_000 }));
    s.tick(T0 + 10_500); // T0 is now 10.5s old → out; T0+9000 is 1.5s old → in
    const [[, stat]] = s.snapshot().topics;
    expect(stat.ratePerSec).toBe(0.1);
    s.tick(T0 + 30_000); // everything out
    expect(s.snapshot().topics[0][1].ratePerSec).toBe(0);
    // count is monotonic — pruning affects only the rate
    expect(s.snapshot().topics[0][1].count).toBe(2);
  });

  it("push prunes its own topic's window using the newest injected time", () => {
    const s = new ObsStore(100);
    s.push(msg({ topic: "t", ts: T0 }));
    s.push(msg({ topic: "t", ts: T0 + 20_000 })); // first ts falls out
    const [[, stat]] = s.snapshot().topics;
    expect(stat.ratePerSec).toBe(0.1);
  });

  it("windows are tracked per topic", () => {
    const s = new ObsStore(100);
    s.push(msg({ topic: "a", ts: T0 }));
    s.push(msg({ topic: "b", ts: T0 + 100 }));
    s.push(msg({ topic: "b", ts: T0 + 200 }));
    const topics = new Map(s.snapshot().topics);
    expect(topics.get("a")!.ratePerSec).toBe(0.1);
    expect(topics.get("b")!.ratePerSec).toBe(0.2);
  });
});

describe("ObsStore snapshot filter", () => {
  const fill = () => {
    const s = new ObsStore(100);
    s.push(msg({ topic: "home/Temp", payload: "21.5", ts: T0 }));
    s.push(msg({ topic: "home/hum", payload: "60 PERCENT", ts: T0 + 1 }));
    s.push(msg({ topic: "garage/door", payload: "open", ts: T0 + 2 }));
    return s;
  };

  it("matches topic case-insensitively", () => {
    const snap = fill().snapshot("temp");
    expect(snap.msgs.map((m) => m.topic)).toEqual(["home/Temp"]);
  });

  it("matches payload case-insensitively", () => {
    const snap = fill().snapshot("percent");
    expect(snap.msgs.map((m) => m.topic)).toEqual(["home/hum"]);
  });

  it("matches topic OR payload", () => {
    const snap = fill().snapshot("o");
    // "home/..." topics contain 'o'; "open" payload contains 'o'
    expect(snap.msgs.length).toBe(3);
  });

  it("empty/undefined filter returns everything; topics stay sorted by name", () => {
    const s = fill();
    expect(s.snapshot().msgs.length).toBe(3);
    expect(s.snapshot("").msgs.length).toBe(3);
    expect(s.snapshot("temp").topics.map(([n]) => n)).toEqual([
      "garage/door",
      "home/Temp",
      "home/hum",
    ]);
  });

  it("no match → empty msgs, filter never mutates the store", () => {
    const s = fill();
    expect(s.snapshot("zzz").msgs).toEqual([]);
    expect(s.snapshot().msgs.length).toBe(3);
  });
});

describe("ObsStore version discipline", () => {
  it("push bumps version (also while paused)", () => {
    const s = new ObsStore(10);
    const v0 = s.version;
    s.push(msg());
    expect(s.version).toBe(v0 + 1);
    s.setPaused(true);
    const v1 = s.version;
    s.push(msg());
    expect(s.version).toBe(v1 + 1);
  });

  it("no-op tick does not bump version", () => {
    const s = new ObsStore(10);
    s.push(msg({ ts: T0 }));
    const v = s.version;
    s.tick(T0 + 1); // nothing leaves the window → no rate change
    expect(s.version).toBe(v);
    s.tick(); // no time injected → no change either
    expect(s.version).toBe(v);
  });

  it("tick bumps version only when a rate changes", () => {
    const s = new ObsStore(10);
    s.push(msg({ ts: T0 }));
    const v = s.version;
    s.tick(T0 + 15_000); // message drops out of the window
    expect(s.version).toBe(v + 1);
    const v2 = s.version;
    s.tick(T0 + 16_000); // already empty → no change
    expect(s.version).toBe(v2);
  });

  it("setPaused bumps only on state change; clear bumps", () => {
    const s = new ObsStore(10);
    const v0 = s.version;
    s.setPaused(false); // already unpaused → no-op
    expect(s.version).toBe(v0);
    s.setPaused(true);
    expect(s.version).toBe(v0 + 1);
    s.setPaused(true); // no-op
    expect(s.version).toBe(v0 + 1);
    s.setPaused(false);
    expect(s.version).toBe(v0 + 2);
    s.clear();
    expect(s.version).toBe(v0 + 3);
  });

  it("snapshot never bumps version", () => {
    const s = new ObsStore(10);
    s.push(msg());
    const v = s.version;
    s.snapshot();
    s.snapshot("x");
    expect(s.version).toBe(v);
  });
});
