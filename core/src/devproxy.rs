//! Pure helpers for the device-browser proxy.
//!
//! The proxy itself (a loopback `tiny_http` listener forwarding to one
//! bench device) lives in src-tauri beside the MCP listener it mirrors;
//! what lives here is everything that can be tested without a socket:
//! target-URL validation, hop-by-hop header classification, and the
//! body previews the request log shows.

use crate::{Error, Result};

/// A validated proxy target: a plain-http origin on the bench LAN.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub host: String,
    pub port: u16,
}

/// Validate and normalize a device base URL.
///
/// Accepts `http://host`, `http://host:port`, with or without a lone
/// trailing `/`. Anything else is refused with a message that says what
/// to change rather than what failed to parse: the target is an *origin*
/// — a path would silently vanish from every proxied request, and https
/// is refused because the forwarder speaks plain http (bench devices
/// don't do TLS, and pretending otherwise would mislead).
pub fn parse_target(raw: &str) -> Result<Target> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::Other("enter a device URL, e.g. http://unoq.local".into()));
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        let _ = rest;
        return Err(Error::Other(
            "the device proxy speaks plain http — use http:// (bench devices don't serve TLS)"
                .into(),
        ));
    }
    let rest = trimmed.strip_prefix("http://").ok_or_else(|| {
        Error::Other("a device URL starts with http:// , e.g. http://unoq.local".into())
    })?;
    let origin = rest.strip_suffix('/').unwrap_or(rest);
    if origin.contains('/') {
        return Err(Error::Other(
            "a device target is an origin — drop the path (it would vanish from proxied requests)"
                .into(),
        ));
    }
    if origin.is_empty() {
        return Err(Error::Other("a device URL needs a host".into()));
    }
    let (host, port) = match origin.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => {
            let port: u16 = p.parse().map_err(|_| {
                Error::Other(format!("`{p}` is not a port number"))
            })?;
            (h.to_string(), port)
        }
        _ => (origin.to_string(), 80),
    };
    if host.is_empty() || host.contains(|c: char| c.is_whitespace()) {
        return Err(Error::Other("a device URL needs a host".into()));
    }
    Ok(Target { host, port })
}

/// Hop-by-hop headers (RFC 9110 §7.6.1) that must not be forwarded in
/// either direction — they describe one connection, not the message.
pub fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "proxy-connection"
    )
}

/// A log-friendly preview of a body: up to `max` bytes, cut on a char
/// boundary when the body is valid UTF-8. Binary bodies (invalid UTF-8 or
/// containing NUL) return a hex-dump-ready marker instead of mojibake —
/// the UI decides how to render, this only classifies. Returns
/// `(preview, truncated)`.
pub fn body_preview(body: &[u8], max: usize) -> (String, bool) {
    match std::str::from_utf8(body) {
        Ok(text) if !text.contains('\u{0}') => {
            if text.len() <= max {
                (text.to_string(), false)
            } else {
                // Largest char boundary at or below max.
                let mut cut = max;
                while cut > 0 && !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                (text[..cut].to_string(), true)
            }
        }
        _ => {
            // Binary: hex of the first bytes, capped so `max` still bounds
            // the preview's size (3 chars per byte, "xx " groups).
            let take = (max / 3).min(body.len());
            let hex: String = body[..take]
                .iter()
                .map(|b| format!("{b:02x} "))
                .collect::<String>()
                .trim_end()
                .to_string();
            (hex, take < body.len())
        }
    }
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bare_host_defaulting_to_port_80() {
        assert_eq!(
            parse_target("http://unoq.local").unwrap(),
            Target { host: "unoq.local".into(), port: 80 }
        );
    }

    #[test]
    fn parses_host_port_and_tolerates_a_trailing_slash_and_whitespace() {
        for raw in ["http://192.168.0.7:8080", "http://192.168.0.7:8080/", "  http://192.168.0.7:8080  "] {
            assert_eq!(
                parse_target(raw).unwrap(),
                Target { host: "192.168.0.7".into(), port: 8080 },
                "{raw:?}"
            );
        }
    }

    #[test]
    fn refuses_a_path_with_a_message_that_says_why() {
        let err = parse_target("http://unoq.local/status").unwrap_err().to_string();
        assert!(err.contains("origin"), "got: {err}");
        assert!(err.contains("drop the path"), "got: {err}");
    }

    #[test]
    fn refuses_https_naming_the_constraint() {
        let err = parse_target("https://unoq.local").unwrap_err().to_string();
        assert!(err.contains("plain http"), "got: {err}");
    }

    #[test]
    fn refuses_empty_and_garbage_with_an_example() {
        for bad in ["", "   ", "unoq.local", "ftp://x", "http://", "http:///"] {
            let err = parse_target(bad).unwrap_err().to_string();
            assert!(err.contains("http://") || err.contains("host"), "{bad:?} → {err}");
        }
    }

    #[test]
    fn refuses_a_non_numeric_port() {
        let err = parse_target("http://unoq.local:web").unwrap_err().to_string();
        assert!(err.contains("port"), "got: {err}");
    }

    #[test]
    fn hop_by_hop_covers_the_rfc_list_case_insensitively() {
        for h in [
            "connection", "Keep-Alive", "TE", "trailer", "Transfer-Encoding",
            "upgrade", "Proxy-Authenticate", "proxy-authorization", "Proxy-Connection",
        ] {
            assert!(is_hop_by_hop(h), "{h}");
        }
        for h in ["content-type", "authorization", "host", "content-length"] {
            assert!(!is_hop_by_hop(h), "{h}");
        }
    }

    #[test]
    fn preview_returns_short_utf8_bodies_verbatim() {
        assert_eq!(body_preview(b"{\"t\":24.5}", 64), ("{\"t\":24.5}".into(), false));
    }

    #[test]
    fn preview_cuts_on_a_char_boundary() {
        // "é" is two bytes; a cut at 4 would land mid-char with max 4 on "aaé é".
        let s = "aa\u{e9}\u{e9}"; // a a é é → bytes: 2 + 2 + 2 = 6
        let (p, truncated) = body_preview(s.as_bytes(), 5);
        assert!(truncated);
        assert_eq!(p, "aa\u{e9}"); // cut back from byte 5 (mid-é) to 4
    }

    #[test]
    fn preview_exactly_at_max_is_not_truncated() {
        let (p, truncated) = body_preview(b"abcd", 4);
        assert_eq!((p.as_str(), truncated), ("abcd", false));
    }

    #[test]
    fn binary_bodies_become_bounded_hex() {
        let body = [0u8, 159, 146, 150, 1, 2, 3, 4];
        let (p, truncated) = body_preview(&body, 12); // 12/3 = 4 bytes shown
        assert_eq!(p, "00 9f 92 96");
        assert!(truncated);
    }

    #[test]
    fn utf8_with_a_nul_counts_as_binary() {
        let (p, _) = body_preview(b"ab\0cd", 64);
        assert!(p.starts_with("61 62 00"), "got: {p}");
    }
}
