//! Bancada Scope — pure wire-protocol layer (no I/O, no Tauri).
//!
//! Implements the contracts in `docs/scope-architecture.md`:
//!   §1.1 device binary frames  → [`FrameScanner`] / [`ScopeFrame`]
//!   §1.2 text banner handshake → [`parse_banner`] / [`ScopeCaps`]
//!   §1.3 host→device commands  → [`cmd_id`], [`cmd_start`], [`cmd_stop`], [`cmd_single`]
//!   §2   Rust→frontend Channel envelopes → [`envelope_samples`], [`envelope_json`]
//!
//! All multi-byte integers on the wire are little-endian; the CRC is
//! CRC-16/CCITT-FALSE (poly 0x1021, init 0xFFFF, no reflection, no xor-out).

use serde::{Deserialize, Serialize};

// Re-exported so the Tauri layer can open ports without its own direct
// dependency (the doc places the `serialport` dep in bancada-core).
pub use serialport;

/// Sync bytes preceding every device frame.
pub const SYNC: [u8; 2] = [0xA5, 0x5A];
/// Frame bytes before the payload: sync(2) + type + flags + seq(2) + len(2) + index(4).
pub const HEADER_LEN: usize = 12;
/// Maximum payload length the firmware is allowed to send.
pub const MAX_PAYLOAD: usize = 2048;

/// Channel-envelope kind bytes (contract §2).
pub const ENV_SAMPLES: u8 = 0x01;
pub const ENV_JSON: u8 = 0x02;

/// CRC-16/CCITT-FALSE: poly 0x1021, init 0xFFFF, no reflect, no xor-out.
pub fn crc16_ccitt(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// One validated device frame (contract §1.1), sync/len/CRC stripped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeFrame {
    pub frame_type: u8,
    pub flags: u8,
    pub seq: u16,
    pub first_sample_index: u32,
    pub payload: Vec<u8>,
}

impl ScopeFrame {
    /// u16 ADC samples (bits 0–11 counts, 12–15 channel index).
    pub const TYPE_SAMPLES: u8 = 0x01;
    /// UTF-8 JSON `{"ev":"meta",...}`.
    pub const TYPE_META: u8 = 0x02;
    /// UTF-8 JSON `{"ev":"record",...}` announcing a single-shot record.
    pub const TYPE_RECORD_HDR: u8 = 0x03;
    /// UTF-8 JSON `{"ev":"err","msg":...}`.
    pub const TYPE_ERROR: u8 = 0x7F;

    /// flags bit0: dual-channel capture.
    pub const FLAG_DUAL: u8 = 0x01;
    /// flags bit2: last SAMPLES frame of a single-shot record.
    pub const FLAG_LAST_IN_RECORD: u8 = 0x04;
    /// flags bit3: driver overflow occurred since last frame.
    pub const FLAG_OVERFLOW: u8 = 0x08;
}

/// Incremental scanner for the device byte stream.
///
/// Feed arbitrary chunks with [`push`](Self::push); complete, CRC-valid frames
/// come back in order. Garbage and corrupted frames are skipped by re-scanning
/// for the next sync pair one byte forward; a partial frame tail is kept
/// across calls.
#[derive(Debug, Default)]
pub struct FrameScanner {
    buf: Vec<u8>,
    /// Total frames rejected on CRC mismatch since construction.
    pub crc_errors: u32,
    last_seq: Option<u16>,
    dropped: u32,
}

impl FrameScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Frames lost since the last call, per seq arithmetic (u16 wraparound).
    pub fn take_dropped(&mut self) -> u32 {
        std::mem::take(&mut self.dropped)
    }

    /// Append `bytes` and return every complete valid frame found so far.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<ScopeFrame> {
        self.buf.extend_from_slice(bytes);
        let mut frames = Vec::new();
        let mut pos = 0usize;

        loop {
            let Some(start) = find_sync(&self.buf, pos) else {
                // No sync pair ahead; keep a trailing 0xA5 (could be half a sync).
                pos = if self.buf.last() == Some(&SYNC[0]) {
                    self.buf.len() - 1
                } else {
                    self.buf.len()
                };
                break;
            };
            let avail = self.buf.len() - start;
            if avail < HEADER_LEN {
                pos = start; // wait for the rest of the header
                break;
            }
            let payload_len =
                u16::from_le_bytes([self.buf[start + 6], self.buf[start + 7]]) as usize;
            if payload_len > MAX_PAYLOAD {
                pos = start + 1; // bogus sync — scan forward
                continue;
            }
            let total = HEADER_LEN + payload_len + 2;
            if avail < total {
                pos = start; // wait for payload + CRC
                break;
            }
            let crc_calc = crc16_ccitt(&self.buf[start + 2..start + HEADER_LEN + payload_len]);
            let crc_wire = u16::from_le_bytes([
                self.buf[start + HEADER_LEN + payload_len],
                self.buf[start + HEADER_LEN + payload_len + 1],
            ]);
            if crc_calc != crc_wire {
                self.crc_errors += 1;
                pos = start + 1; // resync
                continue;
            }

            let seq = u16::from_le_bytes([self.buf[start + 4], self.buf[start + 5]]);
            if let Some(prev) = self.last_seq {
                self.dropped += seq.wrapping_sub(prev).wrapping_sub(1) as u32;
            }
            self.last_seq = Some(seq);

            frames.push(ScopeFrame {
                frame_type: self.buf[start + 2],
                flags: self.buf[start + 3],
                seq,
                first_sample_index: u32::from_le_bytes([
                    self.buf[start + 8],
                    self.buf[start + 9],
                    self.buf[start + 10],
                    self.buf[start + 11],
                ]),
                payload: self.buf[start + HEADER_LEN..start + HEADER_LEN + payload_len].to_vec(),
            });
            pos = start + total;
        }

        self.buf.drain(..pos);
        frames
    }
}

/// First offset ≥ `from` where the sync pair starts, if any.
fn find_sync(buf: &[u8], from: usize) -> Option<usize> {
    if buf.len() < 2 {
        return None;
    }
    (from..buf.len() - 1).find(|&i| buf[i] == SYNC[0] && buf[i + 1] == SYNC[1])
}

// ---------- banner (contract §1.2) ----------

/// Capabilities announced by the firmware banner. `baud` is not part of the
/// banner JSON — the caller fills in the baud that worked (0 = unknown/CDC).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeCaps {
    #[serde(default)]
    pub baud: u32,
    pub proto: u32,
    pub chip: String,
    pub fw: String,
    pub max_sps: u32,
    pub max_stream_sps: u32,
    pub pins: Vec<u8>,
    pub atten: Vec<u8>,
    pub maxch: u8,
}

/// Parse a `!BANCADA {json}` banner line. Returns `None` unless the line has
/// the prefix and well-formed JSON with all required fields.
pub fn parse_banner(line: &str) -> Option<ScopeCaps> {
    let json = line.trim().strip_prefix("!BANCADA ")?;
    serde_json::from_str(json).ok()
}

// ---------- control commands (contract §1.3) ----------

/// Continuous-stream configuration (`scope_start` payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeStreamCfg {
    pub sps: u32,
    pub pins: Vec<u8>,
    pub atten: u8,
}

/// Single-shot (device-triggered burst) configuration (`scope_single` payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeSingleCfg {
    pub sps: u32,
    pub pin: u8,
    pub atten: u8,
    pub pre: u32,
    pub post: u32,
    pub level: u16,
    pub edge: String,
}

pub fn cmd_id() -> String {
    "{\"c\":\"id\"}\n".to_string()
}

pub fn cmd_stop() -> String {
    "{\"c\":\"stop\"}\n".to_string()
}

pub fn cmd_start(sps: u32, pins: &[u8], atten: u8) -> String {
    format!(
        "{{\"c\":\"start\",\"sps\":{sps},\"pins\":{},\"atten\":{atten}}}\n",
        serde_json::to_string(pins).expect("serializing a byte slice cannot fail"),
    )
}

pub fn cmd_single(cfg: &ScopeSingleCfg) -> String {
    format!(
        "{{\"c\":\"single\",\"sps\":{},\"pin\":{},\"atten\":{},\"pre\":{},\"post\":{},\"level\":{},\"edge\":{}}}\n",
        cfg.sps,
        cfg.pin,
        cfg.atten,
        cfg.pre,
        cfg.post,
        cfg.level,
        serde_json::to_string(&cfg.edge).expect("serializing a string cannot fail"),
    )
}

// ---------- host → frontend Channel envelopes (contract §2) ----------

/// Samples envelope: `u8 kind=0x01, u8 flags, u32le first_sample_index,
/// u16le nsamples, u16[nsamples] raw sample bytes verbatim`.
pub fn envelope_samples(flags: u8, first_sample_index: u32, payload: &[u8]) -> Vec<u8> {
    let nsamples = (payload.len() / 2) as u16;
    let mut out = Vec::with_capacity(8 + payload.len());
    out.push(ENV_SAMPLES);
    out.push(flags);
    out.extend_from_slice(&first_sample_index.to_le_bytes());
    out.extend_from_slice(&nsamples.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// JSON envelope: `u8 kind=0x02` followed by the UTF-8 event text.
pub fn envelope_json(json: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + json.len());
    out.push(ENV_JSON);
    out.extend_from_slice(json.as_bytes());
    out
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a wire frame per contract §1.1 (sync + header + payload + CRC).
    fn build_frame(ty: u8, flags: u8, seq: u16, index: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![SYNC[0], SYNC[1], ty, flags];
        v.extend_from_slice(&seq.to_le_bytes());
        v.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        v.extend_from_slice(&index.to_le_bytes());
        v.extend_from_slice(payload);
        let crc = crc16_ccitt(&v[2..]);
        v.extend_from_slice(&crc.to_le_bytes());
        v
    }

    /// Little-endian u16 samples with channel nibbles, no 0xA5 bytes anywhere.
    fn sample_payload() -> Vec<u8> {
        let samples: [u16; 4] = [0x0123, 0x0456, 0x1789, 0x1ABC];
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn crc_known_vector() {
        assert_eq!(crc16_ccitt(b"123456789"), 0x29B1);
        assert_eq!(crc16_ccitt(b""), 0xFFFF);
    }

    #[test]
    fn valid_samples_frame_round_trip() {
        let payload = sample_payload();
        let wire = build_frame(ScopeFrame::TYPE_SAMPLES, 0x01, 7, 4096, &payload);
        let mut sc = FrameScanner::new();
        let frames = sc.push(&wire);
        assert_eq!(frames.len(), 1);
        let f = &frames[0];
        assert_eq!(f.frame_type, ScopeFrame::TYPE_SAMPLES);
        assert_eq!(f.flags, 0x01);
        assert_eq!(f.seq, 7);
        assert_eq!(f.first_sample_index, 4096);
        assert_eq!(f.payload, payload);
        assert_eq!(sc.crc_errors, 0);
        assert_eq!(sc.take_dropped(), 0);
    }

    #[test]
    fn frame_split_across_two_pushes() {
        let wire = build_frame(ScopeFrame::TYPE_META, 0, 1, 0, br#"{"ev":"meta"}"#);
        let mut sc = FrameScanner::new();
        assert!(sc.push(&wire[..9]).is_empty());
        let frames = sc.push(&wire[9..]);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, br#"{"ev":"meta"}"#);
    }

    #[test]
    fn garbage_prefix_resync() {
        let mut stream = b"boot noise \xA5 not-a-frame\n".to_vec();
        stream.extend_from_slice(&build_frame(
            ScopeFrame::TYPE_SAMPLES,
            0,
            3,
            8,
            &sample_payload(),
        ));
        let mut sc = FrameScanner::new();
        let frames = sc.push(&stream);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].seq, 3);
        assert_eq!(sc.crc_errors, 0);
    }

    #[test]
    fn corrupted_crc_skipped_and_counted() {
        let mut bad = build_frame(ScopeFrame::TYPE_SAMPLES, 0, 1, 0, &sample_payload());
        let last = bad.len() - 1;
        bad[last] ^= 0xFF; // corrupt CRC
        let good = build_frame(ScopeFrame::TYPE_SAMPLES, 0, 2, 4, &sample_payload());
        let mut stream = bad;
        stream.extend_from_slice(&good);
        let mut sc = FrameScanner::new();
        let frames = sc.push(&stream);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].seq, 2);
        assert_eq!(sc.crc_errors, 1);
        // First frame ever seen — no drop is inferred from its seq.
        assert_eq!(sc.take_dropped(), 0);
    }

    #[test]
    fn seq_wraparound_counts_dropped() {
        let a = build_frame(ScopeFrame::TYPE_SAMPLES, 0, 0xFFFF, 0, &sample_payload());
        let b = build_frame(ScopeFrame::TYPE_SAMPLES, 0, 0x0001, 16, &sample_payload());
        let mut sc = FrameScanner::new();
        sc.push(&a);
        sc.push(&b);
        assert_eq!(sc.take_dropped(), 1); // seq 0x0000 went missing across the wrap
        assert_eq!(sc.take_dropped(), 0); // counter is consumed
    }

    #[test]
    fn contiguous_seq_drops_nothing() {
        let mut sc = FrameScanner::new();
        for seq in [41u16, 42, 43] {
            sc.push(&build_frame(
                ScopeFrame::TYPE_SAMPLES,
                0,
                seq,
                0,
                &sample_payload(),
            ));
        }
        assert_eq!(sc.take_dropped(), 0);
    }

    #[test]
    fn oversized_payload_len_treated_as_garbage() {
        // Sync pair followed by a header claiming a 3000-byte payload, then a
        // real frame: the scanner must skip the impostor and find the frame.
        let mut stream = vec![0xA5, 0x5A, 0x01, 0x00, 0x00, 0x00, 0xB8, 0x0B, 0, 0, 0, 0];
        stream.extend_from_slice(&build_frame(
            ScopeFrame::TYPE_ERROR,
            0,
            9,
            0,
            br#"{"ev":"err","msg":"x"}"#,
        ));
        let mut sc = FrameScanner::new();
        let frames = sc.push(&stream);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].frame_type, ScopeFrame::TYPE_ERROR);
    }

    #[test]
    fn banner_parses_good_line() {
        let line = r#"!BANCADA {"proto":1,"chip":"esp32s3","fw":"0.1.0","max_sps":83333,"max_stream_sps":83333,"pins":[1,2],"atten":[0,1,2,3],"maxch":2}"#;
        let caps = parse_banner(line).expect("banner should parse");
        assert_eq!(caps.proto, 1);
        assert_eq!(caps.chip, "esp32s3");
        assert_eq!(caps.fw, "0.1.0");
        assert_eq!(caps.max_sps, 83333);
        assert_eq!(caps.max_stream_sps, 83333);
        assert_eq!(caps.pins, vec![1, 2]);
        assert_eq!(caps.atten, vec![0, 1, 2, 3]);
        assert_eq!(caps.maxch, 2);
        assert_eq!(caps.baud, 0); // filled by the caller
    }

    #[test]
    fn banner_rejects_bad_lines() {
        assert!(parse_banner("hello world").is_none());
        assert!(parse_banner("!BANCADA not json").is_none());
        assert!(parse_banner(r#"!BANCADA {"proto":1}"#).is_none()); // missing fields
    }

    #[test]
    fn command_builders_match_contract() {
        assert_eq!(cmd_id(), "{\"c\":\"id\"}\n");
        assert_eq!(cmd_stop(), "{\"c\":\"stop\"}\n");
        assert_eq!(
            cmd_start(50000, &[1], 3),
            "{\"c\":\"start\",\"sps\":50000,\"pins\":[1],\"atten\":3}\n"
        );
        let single = ScopeSingleCfg {
            sps: 1_000_000,
            pin: 34,
            atten: 3,
            pre: 2048,
            post: 6144,
            level: 2048,
            edge: "r".to_string(),
        };
        assert_eq!(
            cmd_single(&single),
            "{\"c\":\"single\",\"sps\":1000000,\"pin\":34,\"atten\":3,\"pre\":2048,\"post\":6144,\"level\":2048,\"edge\":\"r\"}\n"
        );
    }

    #[test]
    fn envelopes_match_contract() {
        let samples = [0x34u8, 0x12, 0x78, 0x56];
        let env = envelope_samples(0x05, 0xDEADBEEF, &samples);
        let mut expect = vec![ENV_SAMPLES, 0x05];
        expect.extend_from_slice(&0xDEADBEEFu32.to_le_bytes());
        expect.extend_from_slice(&2u16.to_le_bytes());
        expect.extend_from_slice(&samples);
        assert_eq!(env, expect);

        let env = envelope_json(r#"{"ev":"closed"}"#);
        assert_eq!(env[0], ENV_JSON);
        assert_eq!(&env[1..], br#"{"ev":"closed"}"#);
    }
}
