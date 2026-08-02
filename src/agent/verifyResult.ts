// Parser for the `mcp__bancada__verify` tool result text.
//
// This is one half of a cross-language contract: `run_verify` in
// `src-tauri/src/lib.rs` formats every successful tool call as
//
//     success: <bool>\nexit_code: <n>\n\n<summary>
//
// and this file is the only thing that reads it back. It lived inline in
// AgentPanel.tsx, where the contract was held together by nothing but two
// comments pointing at each other; extracted here (mirroring `diff.ts` and
// `fences.ts`) so it can be tested against the exact strings the Rust side
// emits instead of only through a rendered component.
//
// Tolerant by policy: the wire format can change under us (the CLI is not
// the only thing between the two ends — see `agentStore`'s `toResultText`
// for the content-block shape question), so an unrecognised shape yields
// `success: undefined` and the raw text as the summary rather than throwing
// or, worse, confidently reporting a pass.

export interface VerifyResult {
  /** `undefined` when the text did not parse as a verify result at all. */
  success?: boolean;
  exitCode?: number;
  /** The build output, or the whole raw text when nothing parsed. */
  summary: string;
}

const SUCCESS_LINE = /^success:\s*(true|false)\s*$/;
const EXIT_CODE_LINE = /^exit_code:\s*(-?\d+)\s*$/;

/**
 * Parse `run_verify`'s result text.
 *
 * `isError` is deliberately NOT what decides pass/fail: `run_verify`
 * reserves it for "the tool could not run at all" (arduino-cli missing, the
 * build gate busy, the session stopped) and reports a *failed build* with
 * `isError: false`, because a fix-the-build loop is the normal path of the
 * feature and flagging it as a tool failure invites the model to give up on
 * the tool. So a completed verify's pass/fail lives here, in the text.
 */
export function parseVerifyResult(result: string): VerifyResult {
  const lines = result.split("\n");
  const successMatch = SUCCESS_LINE.exec(lines[0] ?? "");
  if (!successMatch) return { summary: result };

  let bodyStart = 1;
  let exitCode: number | undefined;
  const exitMatch = EXIT_CODE_LINE.exec(lines[1] ?? "");
  if (exitMatch) {
    exitCode = Number(exitMatch[1]);
    bodyStart = 2;
  }
  while (bodyStart < lines.length && lines[bodyStart] === "") bodyStart++;

  return {
    success: successMatch[1] === "true",
    exitCode,
    summary: lines.slice(bodyStart).join("\n"),
  };
}
