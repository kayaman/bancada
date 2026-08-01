import { describe, expect, it } from "vitest";
import { splitFences } from "../fences";

describe("splitFences", () => {
  it("no fences: the whole text is one text segment", () => {
    const text = "just plain text\nwith two lines";
    expect(splitFences(text)).toEqual([{ kind: "text", content: text }]);
  });

  it("one fenced block with a language tag, surrounded by text", () => {
    const text = "before\n```js\nconst x = 1;\n```\nafter";
    expect(splitFences(text)).toEqual([
      { kind: "text", content: "before" },
      { kind: "code", lang: "js", content: "const x = 1;" },
      { kind: "text", content: "after" },
    ]);
  });

  it("a fence with no language tag omits `lang`", () => {
    const text = "```\nplain code\n```";
    expect(splitFences(text)).toEqual([{ kind: "code", content: "plain code" }]);
  });

  it("multiple blocks interleaved with text", () => {
    const text = "intro\n```py\nprint(1)\n```\nmiddle\n```sh\necho hi\n```\noutro";
    expect(splitFences(text)).toEqual([
      { kind: "text", content: "intro" },
      { kind: "code", lang: "py", content: "print(1)" },
      { kind: "text", content: "middle" },
      { kind: "code", lang: "sh", content: "echo hi" },
      { kind: "text", content: "outro" },
    ]);
  });

  it("an unclosed trailing fence treats the rest of the text as code", () => {
    const text = "before\n```\ncode here\nmore code";
    expect(splitFences(text)).toEqual([
      { kind: "text", content: "before" },
      { kind: "code", content: "code here\nmore code" },
    ]);
  });

  it("an unclosed fence with a language tag and no preceding text", () => {
    const text = "```ts\nconst x: number = 1;";
    expect(splitFences(text)).toEqual([
      { kind: "code", lang: "ts", content: "const x: number = 1;" },
    ]);
  });

  it("empty text yields no segments", () => {
    expect(splitFences("")).toEqual([]);
  });

  it("an empty code block between fences yields empty content, not omitted", () => {
    const text = "```\n```";
    expect(splitFences(text)).toEqual([{ kind: "code", content: "" }]);
  });
});
