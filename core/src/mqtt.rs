//! MQTT observability client — pure protocol/config layer (no sockets, no
//! Tauri). The Tauri layer drives `rumqttc`'s sync `Client`/`Connection`;
//! everything testable lives here:
//!
//!   broker URL parsing    → [`parse_url`] / [`BrokerAddr`]
//!   credential redaction  → [`redact_password`]
//!   topic filter matching → [`topic_matches`] (`+`/`#` rules)
//!   `mqtt.json` config    → [`MqttConfig`] / [`load`] / [`save`]
//!   Rust→frontend Channel envelopes (JSON, tag `"ev"`) → [`MqttEvent`]
//!
//! Envelope contract (exact wire shapes, see tests):
//!   {"ev":"stage","stage":"parse|tcp|connack|suback","ok":true,"detail":"…","elapsed_ms":12}
//!   {"ev":"msg","topic":"…","payload":"…","b64":false,"retain":false,"qos":0,"ts":1753400000000}
//!   {"ev":"closed","reason":"…"}
//!
//! Payloads that are valid UTF-8 cross as-is (`b64:false`); anything else is
//! base64-encoded with `b64:true`.

use std::path::Path;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::Result;

// Re-exported so the Tauri layer drives the broker connection without its own
// direct dependency (same pattern as `scope::serialport`).
pub use rumqttc;

/// MQTT port used when the URL names none.
pub const DEFAULT_PORT: u16 = 1883;

// ---------- broker URL ----------

/// A parsed `mqtt://[user[:pass]@]host[:port]` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerAddr {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Hand-parse a broker URL. Only the `mqtt://` scheme is accepted; the port
/// defaults to 1883. Errors are human-readable — they go straight to the UI.
pub fn parse_url(url: &str) -> std::result::Result<BrokerAddr, String> {
    let url = url.trim();
    let Some(rest) = url.strip_prefix("mqtt://") else {
        return Err(format!(
            "broker URL must start with mqtt:// (got {url:?})"
        ));
    };

    // The last '@' separates userinfo from host, so passwords may contain '@'.
    let (userinfo, hostport) = match rest.rfind('@') {
        Some(at) => (Some(&rest[..at]), &rest[at + 1..]),
        None => (None, rest),
    };

    let (username, password) = match userinfo {
        None => (None, None),
        Some(ui) => match ui.split_once(':') {
            Some((u, p)) => (Some(u.to_string()), Some(p.to_string())),
            None => (Some(ui.to_string()), None),
        },
    };
    if matches!(&username, Some(u) if u.is_empty()) {
        return Err("broker URL has an empty user name before '@'".to_string());
    }

    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p
                .parse()
                .map_err(|_| format!("invalid port {p:?} in broker URL"))?;
            (h, port)
        }
        None => (hostport, DEFAULT_PORT),
    };
    if host.is_empty() {
        return Err("broker URL has no host".to_string());
    }

    Ok(BrokerAddr {
        host: host.to_string(),
        port,
        username,
        password,
    })
}

/// Replace the password in `scheme://user:pass@…` with `***`. URLs without a
/// password come back untouched. Scheme-agnostic on purpose (the frontend twin
/// also redacts `ws://`).
pub fn redact_password(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let rest = &url[scheme_end + 3..];
    let Some(at) = rest.rfind('@') else {
        return url.to_string();
    };
    let userinfo = &rest[..at];
    let Some(colon) = userinfo.find(':') else {
        return url.to_string();
    };
    format!(
        "{}***{}",
        &url[..scheme_end + 3 + colon + 1],
        &rest[at..]
    )
}

// ---------- topic filters ----------

/// MQTT topic-filter match: `+` matches exactly one level, `#` (last level
/// only) matches the rest of the topic including the parent level.
pub fn topic_matches(filter: &str, topic: &str) -> bool {
    let mut f = filter.split('/');
    let mut t = topic.split('/');
    loop {
        match (f.next(), t.next()) {
            (Some("#"), _) => return true,
            (Some("+"), Some(_)) => {}
            (Some(fl), Some(tl)) if fl == tl => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

// ---------- channel envelopes ----------

/// Connection-diagnostic stages, in the order the UI shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MqttStage {
    Parse,
    Tcp,
    Connack,
    Suback,
}

/// One JSON event on the `mqtt_connect` channel (contract in module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "lowercase")]
pub enum MqttEvent {
    Stage {
        stage: MqttStage,
        ok: bool,
        detail: String,
        elapsed_ms: u64,
    },
    Msg {
        topic: String,
        payload: String,
        b64: bool,
        retain: bool,
        qos: u8,
        ts: u64,
    },
    Closed {
        reason: String,
    },
}

impl MqttEvent {
    /// Build a `msg` event from a raw publish: UTF-8 payloads pass through
    /// (`b64:false`), anything else is base64-encoded (`b64:true`).
    pub fn msg_from_parts(topic: &str, payload: &[u8], retain: bool, qos: u8, ts: u64) -> Self {
        let (payload, b64) = match std::str::from_utf8(payload) {
            Ok(s) => (s.to_string(), false),
            Err(_) => (
                base64::engine::general_purpose::STANDARD.encode(payload),
                true,
            ),
        };
        MqttEvent::Msg {
            topic: topic.to_string(),
            payload,
            b64,
            retain,
            qos,
            ts,
        }
    }
}

/// Human-readable text for a CONNACK return code (MQTT 3.1.1 names).
pub fn connack_text(code: rumqttc::ConnectReturnCode) -> &'static str {
    use rumqttc::ConnectReturnCode as C;
    match code {
        C::Success => "connection accepted",
        C::RefusedProtocolVersion => "unacceptable protocol version",
        C::BadClientId => "identifier rejected",
        C::ServiceUnavailable => "server unavailable",
        C::BadUserNamePassword => "bad user name or password",
        C::NotAuthorized => "not authorized",
    }
}

/// `bancada-` plus 4 hex digits derived from the clock — unique enough for a
/// broker to tell concurrent Bancadas apart, with no rand dependency.
pub fn client_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // SplitMix64-style scramble so consecutive launches differ in every bit.
    let mut h = nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 33;
    format!("bancada-{:04x}", (h & 0xFFFF) as u16)
}

// ---------- mqtt.json config ----------

/// One saved broker preset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttBrokerCfg {
    pub name: String,
    pub url: String,
}

/// Persisted broker presets (`mqtt.json`). Path-agnostic like `settings`:
/// the Tauri layer decides where the file lives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MqttConfig {
    #[serde(default)]
    pub brokers: Vec<MqttBrokerCfg>,
}

impl Default for MqttConfig {
    fn default() -> Self {
        MqttConfig {
            brokers: vec![MqttBrokerCfg {
                name: "Pi broker".to_string(),
                url: "mqtt://iot:iot@192.168.15.6:1883".to_string(),
            }],
        }
    }
}

/// Loading never fails — a missing or corrupt file yields the default preset,
/// because config must never block the panel from opening.
pub fn load(path: &Path) -> MqttConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Atomic write (temp file + rename) so a crash mid-write can't corrupt.
pub fn save(path: &Path, cfg: &MqttConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(cfg).expect("MqttConfig always serializes");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- parse_url -----

    #[test]
    fn url_full_form() {
        let a = parse_url("mqtt://iot:iot@192.168.15.6:1883").unwrap();
        assert_eq!(a.host, "192.168.15.6");
        assert_eq!(a.port, 1883);
        assert_eq!(a.username.as_deref(), Some("iot"));
        assert_eq!(a.password.as_deref(), Some("iot"));
    }

    #[test]
    fn url_host_only_defaults_port() {
        let a = parse_url("mqtt://broker.local").unwrap();
        assert_eq!(a.host, "broker.local");
        assert_eq!(a.port, DEFAULT_PORT);
        assert_eq!(a.username, None);
        assert_eq!(a.password, None);
    }

    #[test]
    fn url_host_port_no_credentials() {
        let a = parse_url("mqtt://10.0.0.2:8883").unwrap();
        assert_eq!(a.host, "10.0.0.2");
        assert_eq!(a.port, 8883);
        assert_eq!(a.username, None);
    }

    #[test]
    fn url_user_without_password() {
        let a = parse_url("mqtt://alice@host").unwrap();
        assert_eq!(a.username.as_deref(), Some("alice"));
        assert_eq!(a.password, None);
        assert_eq!(a.host, "host");
    }

    #[test]
    fn url_password_may_contain_at_and_colon() {
        let a = parse_url("mqtt://u:p@ss:w@rd@host:2000").unwrap();
        assert_eq!(a.username.as_deref(), Some("u"));
        assert_eq!(a.password.as_deref(), Some("p@ss:w@rd"));
        assert_eq!(a.host, "host");
        assert_eq!(a.port, 2000);
    }

    #[test]
    fn url_trims_whitespace() {
        let a = parse_url("  mqtt://host  ").unwrap();
        assert_eq!(a.host, "host");
    }

    #[test]
    fn url_rejects_wrong_scheme() {
        let e = parse_url("tcp://host").unwrap_err();
        assert!(e.contains("mqtt://"), "{e}");
        assert!(parse_url("host:1883").is_err());
        assert!(parse_url("").is_err());
    }

    #[test]
    fn url_rejects_bad_port_and_empty_host() {
        let e = parse_url("mqtt://host:abc").unwrap_err();
        assert!(e.contains("port"), "{e}");
        assert!(parse_url("mqtt://host:99999").is_err());
        assert!(parse_url("mqtt://").is_err());
        assert!(parse_url("mqtt://user:pass@").is_err());
        assert!(parse_url("mqtt://@host").is_err());
    }

    // ----- redact_password -----

    #[test]
    fn redact_matrix() {
        assert_eq!(
            redact_password("mqtt://iot:iot@192.168.15.6:1883"),
            "mqtt://iot:***@192.168.15.6:1883"
        );
        assert_eq!(
            redact_password("ws://u:secret@host/path"),
            "ws://u:***@host/path"
        );
        // No password → untouched.
        assert_eq!(redact_password("mqtt://alice@host"), "mqtt://alice@host");
        assert_eq!(redact_password("mqtt://host:1883"), "mqtt://host:1883");
        assert_eq!(redact_password("not a url"), "not a url");
    }

    // ----- topic_matches -----

    #[test]
    fn topic_exact_and_plus() {
        assert!(topic_matches("a/b/c", "a/b/c"));
        assert!(!topic_matches("a/b/c", "a/b"));
        assert!(!topic_matches("a/b", "a/b/c"));
        assert!(topic_matches("a/+/c", "a/b/c"));
        assert!(topic_matches("+/+/+", "a/b/c"));
        assert!(!topic_matches("a/+", "a/b/c")); // + is exactly one level
        assert!(topic_matches("a/+/c", "a//c")); // empty level still a level
    }

    #[test]
    fn topic_hash() {
        assert!(topic_matches("#", "a/b/c"));
        assert!(topic_matches("#", "a"));
        assert!(topic_matches("a/#", "a/b/c"));
        assert!(topic_matches("a/#", "a")); // # includes the parent level
        assert!(!topic_matches("a/#", "b/c"));
        assert!(topic_matches("a/+/#", "a/b"));
        assert!(!topic_matches("a/+/#", "a"));
    }

    // ----- envelopes (exact wire JSON) -----

    #[test]
    fn stage_envelope_exact_json() {
        let ev = MqttEvent::Stage {
            stage: MqttStage::Parse,
            ok: true,
            detail: "…".to_string(),
            elapsed_ms: 12,
        };
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"ev":"stage","stage":"parse","ok":true,"detail":"…","elapsed_ms":12}"#
        );
        for (stage, text) in [
            (MqttStage::Parse, "\"parse\""),
            (MqttStage::Tcp, "\"tcp\""),
            (MqttStage::Connack, "\"connack\""),
            (MqttStage::Suback, "\"suback\""),
        ] {
            assert_eq!(serde_json::to_string(&stage).unwrap(), text);
        }
    }

    #[test]
    fn msg_envelope_exact_json() {
        let ev = MqttEvent::msg_from_parts("bancada/test", b"hi", false, 0, 1753400000000);
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"ev":"msg","topic":"bancada/test","payload":"hi","b64":false,"retain":false,"qos":0,"ts":1753400000000}"#
        );
    }

    #[test]
    fn closed_envelope_exact_json() {
        let ev = MqttEvent::Closed {
            reason: "…".to_string(),
        };
        assert_eq!(
            serde_json::to_string(&ev).unwrap(),
            r#"{"ev":"closed","reason":"…"}"#
        );
    }

    #[test]
    fn msg_binary_payload_is_base64() {
        let ev = MqttEvent::msg_from_parts("t", &[0xFF, 0x00, 0xA5], true, 0, 7);
        let MqttEvent::Msg {
            payload,
            b64,
            retain,
            ..
        } = &ev
        else {
            panic!("expected msg");
        };
        assert!(b64);
        assert!(retain);
        assert_eq!(payload, "/wCl"); // base64 of ff 00 a5
    }

    // ----- connack text -----

    #[test]
    fn connack_texts_are_readable() {
        use rumqttc::ConnectReturnCode as C;
        assert_eq!(connack_text(C::Success), "connection accepted");
        assert_eq!(
            connack_text(C::RefusedProtocolVersion),
            "unacceptable protocol version"
        );
        assert_eq!(connack_text(C::BadClientId), "identifier rejected");
        assert_eq!(connack_text(C::ServiceUnavailable), "server unavailable");
        assert_eq!(
            connack_text(C::BadUserNamePassword),
            "bad user name or password"
        );
        assert_eq!(connack_text(C::NotAuthorized), "not authorized");
    }

    // ----- client id -----

    #[test]
    fn client_id_shape() {
        let id = client_id();
        assert!(id.starts_with("bancada-"), "{id}");
        let hex = &id["bancada-".len()..];
        assert_eq!(hex.len(), 4);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    // ----- config -----

    #[test]
    fn config_default_is_pi_preset() {
        let cfg = MqttConfig::default();
        assert_eq!(cfg.brokers.len(), 1);
        assert_eq!(cfg.brokers[0].name, "Pi broker");
        assert_eq!(cfg.brokers[0].url, "mqtt://iot:iot@192.168.15.6:1883");
    }

    #[test]
    fn config_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("mqtt.json");
        let cfg = MqttConfig {
            brokers: vec![
                MqttBrokerCfg {
                    name: "a".into(),
                    url: "mqtt://a".into(),
                },
                MqttBrokerCfg {
                    name: "b".into(),
                    url: "mqtt://u:p@b:2000".into(),
                },
            ],
        };
        save(&p, &cfg).unwrap();
        assert_eq!(load(&p), cfg);
    }

    #[test]
    fn config_missing_or_corrupt_is_default() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load(&tmp.path().join("nope.json")), MqttConfig::default());
        let p = tmp.path().join("mqtt.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), MqttConfig::default());
    }

    #[test]
    fn config_save_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("deep/nested/mqtt.json");
        save(&p, &MqttConfig::default()).unwrap();
        assert!(p.is_file());
    }
}
