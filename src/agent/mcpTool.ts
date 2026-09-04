// Reading an MCP tool call — the vocabulary behind the Assistant log's MCP
// card and the activity line's tool hint.
//
// The wire name is already structured: `mcp__<server>__<tool>`. That is not a
// convention this file invents, it is the shape `agent.rs` requires in
// `--allowedTools` and the shape `AgentPanel`'s existing `endsWith("__verify")`
// checks already rely on. So a server's tools can be *decomposed* rather than
// special-cased, and one card serves every server Bancada ever offers —
// Espressif's documentation server being the first that is not our own.
//
// Pure and dependency-free, so the panel holds no logic worth testing.

export interface McpName {
  /** The server as the CLI names it: "espressif-docs", "bancada". */
  server: string;
  /** The tool within it, underscores intact: "search_espressif_sources". */
  tool: string;
}

/**
 * Split an MCP tool name, or `null` for a built-in.
 *
 * Only the DOUBLE underscore separates; a single one is part of a name.
 * `search_espressif_sources` is one tool, not three segments — splitting on
 * `_` would turn every MCP tool into a mess. Anything after the second
 * separator is rejoined, so a tool whose own name contains `__` still names
 * itself correctly rather than being silently truncated.
 */
export function parseMcpName(name: string): McpName | null {
  const parts = name.split("__");
  if (parts.length < 3 || parts[0] !== "mcp") return null;
  const server = parts[1];
  const tool = parts.slice(2).join("__");
  if (server === "" || tool === "") return null;
  return { server, tool };
}

/** The tool without its server prefix — what to call it in a crowded line. */
export function shortToolName(name: string): string {
  return parseMcpName(name)?.tool ?? name;
}

/**
 * The one-line "what was this call about".
 *
 * Ordered by how much the field says about the *subject* rather than the
 * mechanics: a search's `query` first, because that is the entire content of a
 * documentation lookup and the thing a reader scanning a run of them is
 * comparing. Then the fields the activity line already privileges, so the two
 * agree on what matters.
 *
 * Empty for a tool that takes no arguments — `verify` and `upload` take none
 * by design — so the card renders no subject line rather than an honest-looking
 * `{}`.
 */
export function mcpCallSubject(input: unknown): string {
  if (typeof input !== "object" || input === null) return "";
  const i = input as Record<string, unknown>;

  for (const key of ["query", "q", "prompt", "file_path", "command", "pattern", "url"]) {
    const v = i[key];
    if (typeof v === "string" && v.trim() !== "") return v;
  }
  // Nothing named: fall back to the first primitive field, labelled, so a
  // `{gpio: 8}` still reads as something rather than as nothing at all.
  for (const [k, v] of Object.entries(i)) {
    if (typeof v === "string" || typeof v === "number" || typeof v === "boolean") {
      return `${k}: ${v}`;
    }
  }
  return "";
}
