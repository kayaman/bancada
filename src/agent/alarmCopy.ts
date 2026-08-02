// What the panel tells the user when Bancada stops the assistant.
//
// **This is the only safety text a real user ever sees.** Everything else
// about the confinement — the spec, the module headers, the doc comments —
// is read by developers. So it has to be accurate per alarm kind, and it
// must not reassure beyond what the alarm actually establishes.
//
// The first version ended every alarm with a flat "Nothing outside this
// project was changed." That was wrong twice over, which is why this is now
// its own tested module rather than a string literal in JSX.

/** Alarm kinds the host emits (`agent_event_alarm` in src-tauri/src/lib.rs). */
export type AlarmKind = "path_escape" | "unexpected_tools" | (string & {});

/**
 * The consequence line shown under the host's `detail` sentence.
 *
 * Per kind, because the kinds mean genuinely different things:
 *
 * - **`path_escape`** comes from the **layer-4 backstop**: the stdout reader
 *   inspecting an `Edit`/`Write` `tool_use` *after* the model emitted it.
 *   The refusal layers (deny rules, PreToolUse hook) normally stop such a
 *   write before it reaches disk — so if this alarm fired at all, one of
 *   them did not do its job, which is the entire reason the backstop exists.
 *   The write therefore **may have completed**, and claiming otherwise would
 *   be telling the user the opposite of the truth at the one moment it
 *   matters.
 * - **`unexpected_tools`** is about *capabilities*, not files. It says the
 *   session held tools outside the safety model; whether anything was
 *   written is not something that alarm knows either way, so any statement
 *   about files would be invented.
 *
 * Neither branch — and no fallback — offers blanket reassurance.
 */
export function alarmConsequence(kind: AlarmKind): string {
  switch (kind) {
    case "path_escape":
      return (
        "Bancada's write confinement should have refused this before it happened. " +
        "Because this warning came from the after-the-fact check instead, the write " +
        "may have completed — inspect the path above before continuing. The session " +
        "has been stopped."
      );
    case "unexpected_tools":
      return (
        "The session was given capabilities Bancada's safety model does not cover, " +
        "so what it may have done is not bounded by that model. Review the transcript " +
        "above. The session has been stopped."
      );
    default:
      return (
        "Bancada stopped the session for a safety reason it has no specific guidance " +
        "for. Review the transcript above before continuing."
      );
  }
}
