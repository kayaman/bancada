import { beforeEach, describe, expect, it, vi } from "vitest";

// These tests pin the *contract* between the typed wrappers and the Rust
// commands: the command name, and the exact argument keys.
//
// Tauri maps a camelCase key to a snake_case Rust parameter, so `sketchDir`
// reaches `sketch_dir`. Get a key wrong and nothing fails at build time —
// `tsc` is happy, the Rust side compiles, and the mismatch only surfaces at
// runtime as an argument-deserialisation error from a button click. That is
// exactly the class of bug worth a cheap test.

const invokeMock = vi.fn(async (..._args: unknown[]) => undefined);
const listenMock = vi.fn(async (..._args: unknown[]) => () => {});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  // `scopeStart` constructs one; it only needs to exist.
  Channel: class {
    onmessage: unknown = null;
  },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}));

import * as api from "../api";

/** The (command, args) pair of the most recent invoke. */
function called(): [string, Record<string, unknown>] {
  const call = invokeMock.mock.calls.at(-1);
  if (!call) throw new Error("invoke was not called");
  return call as unknown as [string, Record<string, unknown>];
}

beforeEach(() => {
  invokeMock.mockClear();
  listenMock.mockClear();
});

describe("sketch and file commands", () => {
  it("listSketchFiles passes sketchDir", async () => {
    await api.listSketchFiles("/s");
    expect(called()).toEqual(["list_sketch_files", { sketchDir: "/s" }]);
  });

  it("readSketchFile passes sketchDir and relPath", async () => {
    await api.readSketchFile("/s", "src/a.cpp");
    expect(called()).toEqual([
      "read_sketch_file",
      { sketchDir: "/s", relPath: "src/a.cpp" },
    ]);
  });

  it("writeSketchFile passes content alongside the path", async () => {
    await api.writeSketchFile("/s", "a.ino", "void setup() {}");
    expect(called()).toEqual([
      "write_sketch_file",
      { sketchDir: "/s", relPath: "a.ino", content: "void setup() {}" },
    ]);
  });

  it("loadSketchYaml passes sketchDir", async () => {
    await api.loadSketchYaml("/s");
    expect(called()).toEqual(["load_sketch_yaml", { sketchDir: "/s" }]);
  });

  it("createSketchFile passes sketchDir and relPath", async () => {
    await api.createSketchFile("/s", "src/new.cpp");
    expect(called()).toEqual([
      "create_sketch_file",
      { sketchDir: "/s", relPath: "src/new.cpp" },
    ]);
  });

  it("createSketchDir passes sketchDir and relPath", async () => {
    await api.createSketchDir("/s", "assets");
    expect(called()).toEqual([
      "create_sketch_dir",
      { sketchDir: "/s", relPath: "assets" },
    ]);
  });

  it("renameSketchEntry passes from and to", async () => {
    await api.renameSketchEntry("/s", "src/a.cpp", "lib/a.cpp");
    expect(called()).toEqual([
      "rename_sketch_entry",
      { sketchDir: "/s", from: "src/a.cpp", to: "lib/a.cpp" },
    ]);
  });

  it("deleteSketchEntry passes sketchDir and relPath", async () => {
    await api.deleteSketchEntry("/s", "src/a.cpp");
    expect(called()).toEqual([
      "delete_sketch_entry",
      { sketchDir: "/s", relPath: "src/a.cpp" },
    ]);
  });
});

describe("library commands", () => {
  it("addLocalLibrary passes libDir, the local-library differentiator", async () => {
    await api.addLocalLibrary("/s", "esp32s3", "/libs/Foo");
    expect(called()).toEqual([
      "add_local_library",
      { sketchDir: "/s", profile: "esp32s3", libDir: "/libs/Foo" },
    ]);
  });

  it("addRegistryLibraryToProfile keeps its four arguments", async () => {
    await api.addRegistryLibraryToProfile("/s", "p", "ArduinoJson", "7.4.2");
    expect(called()).toEqual([
      "add_registry_library_to_profile",
      { sketchDir: "/s", profile: "p", name: "ArduinoJson", version: "7.4.2" },
    ]);
  });

  it("installLibrary sends an explicit null version rather than omitting it", async () => {
    // Rust takes Option<String>; an absent key and a null both deserialise, but
    // sending null keeps the payload shape stable.
    await api.installLibrary("ArduinoJson");
    expect(called()).toEqual([
      "install_library",
      { name: "ArduinoJson", version: null },
    ]);
  });

  it("installLibrary forwards a pinned version", async () => {
    await api.installLibrary("ArduinoJson", "7.4.2");
    expect(called()).toEqual([
      "install_library",
      { name: "ArduinoJson", version: "7.4.2" },
    ]);
  });

  it("sketchbookLibrariesDir takes no arguments", async () => {
    await api.sketchbookLibrariesDir();
    expect(called()[0]).toBe("sketchbook_libraries_dir");
    expect(called()[1]).toBeUndefined();
  });
});

describe("create library", () => {
  const spec = {
    name: "MyLib",
    version: "0.1.0",
    author: "",
    maintainer: "",
    sentence: "",
    paragraph: "",
    category: "Other",
    url: "",
    architectures: "*",
  };

  it("nests the spec and nulls an absent sketch", async () => {
    await api.createLibrary(spec);
    expect(called()).toEqual([
      "create_library",
      { spec, sketchDir: null, profile: null },
    ]);
  });

  it("forwards the sketch and profile when creating inside a project", async () => {
    await api.createLibrary(spec, "/s", "esp32s3");
    expect(called()).toEqual([
      "create_library",
      { spec, sketchDir: "/s", profile: "esp32s3" },
    ]);
  });

  it("exposes exactly the nine specification categories", () => {
    // Mirrors core's CATEGORIES; "Uncategorized" is the tooling's fallback for
    // an invalid value, never a legal one to declare.
    expect(api.LIBRARY_CATEGORIES).toHaveLength(9);
    expect(api.LIBRARY_CATEGORIES).toContain("Signal Input/Output");
    expect(api.LIBRARY_CATEGORIES).not.toContain("Uncategorized");
  });
});

describe("new project", () => {
  it("createProject passes every field and nulls an omitted profile", async () => {
    await api.createProject("/parent", "Blink", "arduino:avr:uno", null, []);
    expect(called()).toEqual([
      "create_project",
      {
        parent: "/parent",
        name: "Blink",
        fqbn: "arduino:avr:uno",
        profile: null,
        libraries: [],
      },
    ]);
  });

  it("createProject forwards pinned library specs", async () => {
    await api.createProject("/p", "N", "a:b:c", "custom", [
      "ArduinoJson@7.4.2",
      "PubSubClient@2.8",
    ]);
    const [, args] = called();
    expect(args.profile).toBe("custom");
    expect(args.libraries).toEqual(["ArduinoJson@7.4.2", "PubSubClient@2.8"]);
  });

  it("listAllBoards and sketchbookDir take no arguments", async () => {
    await api.listAllBoards();
    expect(called()[0]).toBe("list_all_boards");
    await api.sketchbookDir();
    expect(called()[0]).toBe("sketchbook_dir");
  });
});

describe("git-backed libraries", () => {
  it("ghAddLibrary sends the ref as gitRef, not ref", async () => {
    // The TS parameter is `ref` but Rust's parameter is `git_ref`, so the key
    // must be `gitRef`. Sending `ref` would fail only at runtime.
    await api.ghAddLibrary("/s", "esp32s3", "@o/r/libraries/L", "L/v1.0.0");
    expect(called()).toEqual([
      "gh_add_library",
      {
        sketchDir: "/s",
        profile: "esp32s3",
        alias: "@o/r/libraries/L",
        gitRef: "L/v1.0.0",
      },
    ]);
  });

  it("ghAddLibrary nulls an absent profile", async () => {
    await api.ghAddLibrary("/s", null, "@o/r", "v1");
    expect(called()[1].profile).toBeNull();
  });

  it("ghListVersions passes only the alias", async () => {
    await api.ghListVersions("@o/r/libraries/L");
    expect(called()).toEqual([
      "gh_list_versions",
      { alias: "@o/r/libraries/L" },
    ]);
  });

  it("ghRestore and ghManifest pass the sketch", async () => {
    await api.ghRestore("/s", "p");
    expect(called()).toEqual(["gh_restore", { sketchDir: "/s", profile: "p" }]);
    await api.ghManifest("/s");
    expect(called()).toEqual(["gh_manifest", { sketchDir: "/s" }]);
  });
});

describe("file saving and scope firmware", () => {
  it("saveTextFile passes path and contents", async () => {
    await api.saveTextFile("/tmp/a.csv", "x,y");
    expect(called()).toEqual([
      "save_text_file",
      { path: "/tmp/a.csv", contents: "x,y" },
    ]);
  });

  it("saveBinaryFile sends base64 under contentsB64", async () => {
    await api.saveBinaryFile("/tmp/a.png", "AAAA");
    expect(called()).toEqual([
      "save_binary_file",
      { path: "/tmp/a.png", contentsB64: "AAAA" },
    ]);
  });

  it("scopeInstallFirmware passes destDir", async () => {
    await api.scopeInstallFirmware("/tmp/scratch");
    expect(called()).toEqual([
      "scope_install_firmware",
      { destDir: "/tmp/scratch" },
    ]);
  });
});

describe("events", () => {
  it("subscribes to the exact event names the Rust side emits", async () => {
    // These strings are duplicated in src-tauri; a typo silently means a console
    // that never updates.
    await api.onBuildLine(() => {});
    expect(listenMock.mock.calls.at(-1)?.[0]).toBe("build://line");
    await api.onSerialLine(() => {});
    expect(listenMock.mock.calls.at(-1)?.[0]).toBe("serial://line");
  });
});

describe("agent commands", () => {
  it("agentStart nulls omitted profile/fqbn", async () => {
    await api.agentStart("/s");
    expect(called()).toEqual([
      "agent_start",
      { sketchDir: "/s", profile: null, fqbn: null },
    ]);
  });

  it("agentStart forwards a given profile and fqbn", async () => {
    await api.agentStart("/s", "esp32s3", "esp32:esp32:esp32s3");
    expect(called()).toEqual([
      "agent_start",
      { sketchDir: "/s", profile: "esp32s3", fqbn: "esp32:esp32:esp32s3" },
    ]);
  });

  it("agentSend passes text", async () => {
    await api.agentSend("make this build");
    expect(called()).toEqual(["agent_send", { text: "make this build" }]);
  });

  it("agentProbe and agentInterrupt take no arguments", async () => {
    await api.agentProbe();
    expect(called()[0]).toBe("agent_probe");
    await api.agentInterrupt();
    expect(called()[0]).toBe("agent_interrupt");
  });

  it("agentStop nulls an omitted pid", async () => {
    await api.agentStop();
    expect(called()).toEqual(["agent_stop", { pid: null }]);
  });

  it("agentStop forwards a given pid (F4 guard: agent_stop refuses a stale pid)", async () => {
    await api.agentStop(4242);
    expect(called()).toEqual(["agent_stop", { pid: 4242 }]);
  });

  it("onAgentEvent and onAgentClosed subscribe to the exact event names", async () => {
    await api.onAgentEvent(() => {});
    expect(listenMock.mock.calls.at(-1)?.[0]).toBe("agent://event");
    await api.onAgentClosed(() => {});
    expect(listenMock.mock.calls.at(-1)?.[0]).toBe("agent://closed");
  });
});
