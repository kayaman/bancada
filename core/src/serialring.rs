//! Bounded ring buffer of serial-monitor output for the agent's
//! `serial_read` MCP tool.
//!
//! The monitor reader threads push every line here (in addition to emitting
//! `serial://line` events for the UI); the agent reads "everything since my
//! cursor" in bounded chunks. The ring lives for the whole process — monitor
//! restarts do NOT reset it, and sequence numbers are monotonic across
//! restarts, so a cursor taken before an upload's evict/auto-restart cycle
//! stays unambiguous.
//!
//! Locking: the ring has its own mutex (owned by the caller); nothing joins
//! a thread while holding it, and it is never taken together with the
//! serial-owner mutex. See the concurrency contract in
//! `docs/superpowers/specs/2026-08-09-agent-hardware-web-capabilities-design.md`.

use crate::types::OutputStream;
use std::collections::VecDeque;

/// Default line-count cap: ~a screenful of scrollback per read cycle at the
/// agent's cadence, small enough that the ring is never a memory concern.
pub const DEFAULT_MAX_LINES: usize = 500;
/// Default per-line byte cap: a firmware spewing one unbounded line must not
/// defeat the read-time byte budget.
pub const DEFAULT_MAX_LINE_BYTES: usize = 4096;

struct Entry {
    seq: u64,
    stream: OutputStream,
    line: String,
}

/// What one `read_since` call saw. `new_cursor` is the next unseen sequence
/// number — pass it back to continue; it only advances past what was
/// actually returned, so a byte-budget cut resumes exactly where it stopped.
pub struct ReadResult {
    pub text: String,
    pub new_cursor: u64,
    pub dropped: u64,
}

pub struct SerialRing {
    entries: VecDeque<Entry>,
    next_seq: u64,
    max_lines: usize,
    max_line_bytes: usize,
}

impl Default for SerialRing {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_LINES, DEFAULT_MAX_LINE_BYTES)
    }
}

impl SerialRing {
    pub fn new(max_lines: usize, max_line_bytes: usize) -> Self {
        SerialRing {
            entries: VecDeque::new(),
            next_seq: 0,
            max_lines,
            max_line_bytes,
        }
    }

    /// Append one monitor line, truncating to the per-line cap (on a UTF-8
    /// boundary, with an ellipsis marker) and evicting the oldest line past
    /// the count cap. Sequence numbers are assigned here, under the caller's
    /// lock, and never reset — see the module doc.
    pub fn push(&mut self, stream: OutputStream, line: &str) {
        let line = truncate_utf8(line, self.max_line_bytes);
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.push_back(Entry { seq, stream, line });
        while self.entries.len() > self.max_lines {
            self.entries.pop_front();
        }
    }

    /// The sequence number the *next* pushed line will get. A fresh session
    /// initializes its cursor here so it never replays pre-session backlog.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Everything at or after `cursor`, up to `max_bytes` of rendered text
    /// (always at least one line so progress is guaranteed). `dropped`
    /// counts lines that fell off the window before they were read.
    pub fn read_since(&self, cursor: u64, max_bytes: usize) -> ReadResult {
        let oldest = self.entries.front().map(|e| e.seq).unwrap_or(self.next_seq);
        let effective = cursor.max(oldest);
        let dropped = effective.saturating_sub(cursor).min(self.next_seq.saturating_sub(cursor));

        let mut text = String::new();
        let mut new_cursor = effective;
        for e in self.entries.iter().filter(|e| e.seq >= effective) {
            let rendered = match e.stream {
                OutputStream::Stdout => e.line.clone(),
                OutputStream::Stderr => format!("[stderr] {}", e.line),
            };
            let sep = usize::from(!text.is_empty());
            if !text.is_empty() && text.len() + sep + rendered.len() > max_bytes {
                break;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&rendered);
            new_cursor = e.seq + 1;
        }
        ReadResult { text, new_cursor, dropped }
    }
}

/// Truncate to at most `max` bytes on a char boundary, appending `…` when
/// anything was cut.
fn truncate_utf8(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn ring() -> SerialRing {
        SerialRing::new(4, 32)
    }

    #[test]
    fn read_since_returns_pushed_lines_and_advances_the_cursor() {
        let mut r = ring();
        r.push(OutputStream::Stdout, "boot ok");
        r.push(OutputStream::Stdout, "wifi up");
        let res = r.read_since(0, 1024);
        assert_eq!(res.text, "boot ok\nwifi up");
        assert_eq!(res.dropped, 0);
        // Re-reading from the returned cursor sees nothing new.
        let again = r.read_since(res.new_cursor, 1024);
        assert_eq!(again.text, "");
        assert_eq!(again.dropped, 0);
        assert_eq!(again.new_cursor, res.new_cursor);
    }

    #[test]
    fn sequence_numbers_stay_monotonic_past_the_line_cap() {
        let mut r = ring(); // cap 4
        for i in 0..10 {
            r.push(OutputStream::Stdout, &format!("line{i}"));
        }
        // next_seq keeps counting even though only 4 lines remain.
        assert_eq!(r.next_seq(), 10);
        let res = r.read_since(0, 1024);
        assert_eq!(res.text, "line6\nline7\nline8\nline9");
    }

    #[test]
    fn a_cursor_that_fell_off_the_window_reports_the_dropped_count() {
        let mut r = ring(); // cap 4
        for i in 0..10 {
            r.push(OutputStream::Stdout, &format!("line{i}"));
        }
        // Cursor 2: lines 2..=5 are gone (4 dropped), 6..=9 remain.
        let res = r.read_since(2, 1024);
        assert_eq!(res.dropped, 4);
        assert!(res.text.starts_with("line6"));
    }

    #[test]
    fn an_empty_ring_with_an_old_cursor_counts_everything_as_dropped() {
        let mut r = SerialRing::new(1, 32);
        r.push(OutputStream::Stdout, "a");
        r.push(OutputStream::Stdout, "b"); // "a" evicted
        let res = r.read_since(0, 1024);
        assert_eq!(res.dropped, 1);
        assert_eq!(res.text, "b");
    }

    #[test]
    fn the_byte_budget_stops_mid_backlog_and_the_cursor_resumes_there() {
        let mut r = ring();
        r.push(OutputStream::Stdout, "aaaaaaaaaa"); // 10 bytes
        r.push(OutputStream::Stdout, "bbbbbbbbbb");
        r.push(OutputStream::Stdout, "cccccccccc");
        // Budget fits two lines + separator, not three.
        let first = r.read_since(0, 24);
        assert_eq!(first.text, "aaaaaaaaaa\nbbbbbbbbbb");
        let rest = r.read_since(first.new_cursor, 1024);
        assert_eq!(rest.text, "cccccccccc");
        assert_eq!(rest.dropped, 0);
    }

    #[test]
    fn even_a_tiny_budget_returns_at_least_one_line() {
        let mut r = ring();
        r.push(OutputStream::Stdout, "0123456789");
        let res = r.read_since(0, 3);
        assert_eq!(res.text, "0123456789");
        assert_eq!(res.new_cursor, 1);
    }

    #[test]
    fn overlong_lines_are_truncated_at_push_time() {
        let mut r = ring(); // 32-byte line cap
        let long = "x".repeat(100);
        r.push(OutputStream::Stdout, &long);
        let res = r.read_since(0, 4096);
        assert!(res.text.len() < 100, "line was not truncated: {}", res.text.len());
        assert!(res.text.contains("…"));
    }

    #[test]
    fn truncation_respects_utf8_boundaries() {
        let mut r = SerialRing::new(4, 10);
        // 4-byte emoji straddling the 10-byte cap must not split.
        r.push(OutputStream::Stdout, "12345678💧💧");
        let res = r.read_since(0, 4096);
        assert!(res.text.starts_with("12345678"));
    }

    #[test]
    fn stderr_lines_are_labeled() {
        let mut r = ring();
        r.push(OutputStream::Stdout, "normal");
        r.push(OutputStream::Stderr, "bad news");
        let res = r.read_since(0, 1024);
        assert_eq!(res.text, "normal\n[stderr] bad news");
    }

    #[test]
    fn an_empty_ring_at_head_reads_empty_with_no_drops() {
        let r = SerialRing::default();
        let res = r.read_since(r.next_seq(), 1024);
        assert_eq!(res.text, "");
        assert_eq!(res.dropped, 0);
    }
}
