//! Thin, typed wrapper around `idf.py` — the ESP-IDF build backend.
//!
//! The sibling of [`crate::cli`], and deliberately the same shape: pure argv
//! builders that are unit-tested without running anything, over the shared
//! subprocess plumbing in [`crate::proc`]. We do not reimplement ESP-IDF's
//! build logic any more than we reimplement arduino-cli's.
//!
//! Unlike `arduino-cli`, `idf.py` is not a self-contained executable: it is a
//! Python script that needs an environment (compiler `PATH`, `IDF_PATH`,
//! `ESP_IDF_VERSION`, …) resolved by [`crate::idfenv`] first. That environment
//! is carried on [`IdfCli`] and applied to every child.
//!
//! ## Two things that are *not* true, checked rather than assumed
//!
//! Both were plausible enough to design around, and both are wrong on
//! ESP-IDF v6:
//!
//! - **Global options do not have to precede the action.** `idf.py` is
//!   `click`-based, so `idf.py build -C dir` and `idf.py -C dir build` behave
//!   identically. We emit the canonical order anyway, but nothing depends on
//!   it.
//! - **`flash` already builds first.** `idf_py_actions/serial_ext.py` declares
//!   `'flash': {'dependencies': ['all']}` (and `all` *is* the build), so a
//!   failed compile cannot reach the board and there is no need to spell
//!   `build flash`. This is the one place ESP-IDF is *safer* by default than
//!   arduino-cli, whose bare `upload` would happily flash a stale binary —
//!   the hazard [`crate::cli::upload_args`] exists to avoid.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::types::{OutputLine, RunResult};
use crate::Result;

/// A resolved, runnable ESP-IDF.
#[derive(Debug, Clone)]
pub struct IdfCli {
    /// The venv interpreter this install was set up with.
    pub python: PathBuf,
    /// `IDF_PATH`.
    pub idf_path: PathBuf,
    /// The activated environment, from [`crate::idfenv::activate`].
    pub env: BTreeMap<String, String>,
}

impl IdfCli {
    pub fn new(python: PathBuf, idf_path: PathBuf, env: BTreeMap<String, String>) -> Self {
        Self {
            python,
            idf_path,
            env,
        }
    }

    /// The `idf.py` entry point.
    pub fn idf_py(&self) -> PathBuf {
        self.idf_path.join("tools").join("idf.py")
    }

    fn base_command(&self, args: &[String]) -> Command {
        let mut cmd = Command::new(&self.python);
        cmd.arg(self.idf_py())
            .args(args)
            .envs(&self.env)
            // idf.py is click-based and cmake/ninja colour their output too.
            // Without these, ANSI escapes reach the Build console as literal
            // text, because the console renders lines rather than a terminal.
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// A display string for errors — the command as a human would type it.
    fn display(&self, args: &[String]) -> String {
        format!("idf.py {}", args.join(" "))
    }

    /// Run to completion, streaming stdout+stderr into `on_line`.
    ///
    /// A failing build is a [`RunResult`] with `success: false`, not an `Err`;
    /// only a failure to run `idf.py` at all is an error. Same contract as
    /// [`crate::cli::ArduinoCli::run_streaming`], because the build gate and
    /// the MCP `verify` tool consume both identically.
    pub fn run_streaming(
        &self,
        args: &[String],
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        crate::proc::stream(self.base_command(args), "idf.py", on_line)
    }

    /// Run to completion and return stdout.
    pub fn run_output(&self, args: &[String]) -> Result<String> {
        let display = self.display(args);
        crate::proc::output(self.base_command(args), "idf.py", &display)
    }

    // ---------- operations ----------

    pub fn build(&self, dir: &Path, on_line: impl FnMut(OutputLine)) -> Result<RunResult> {
        self.run_streaming(&build_args(dir), on_line)
    }

    pub fn flash(
        &self,
        dir: &Path,
        port: &str,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        self.run_streaming(&flash_args(dir, port), on_line)
    }

    pub fn set_target(
        &self,
        dir: &Path,
        target: &str,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        self.run_streaming(&set_target_args(dir, target), on_line)
    }

    pub fn clean(
        &self,
        dir: &Path,
        full: bool,
        on_line: impl FnMut(OutputLine),
    ) -> Result<RunResult> {
        self.run_streaming(&clean_args(dir, full), on_line)
    }

    /// The chip targets this ESP-IDF supports.
    pub fn list_targets(&self) -> Result<Vec<String>> {
        Ok(parse_list_targets(&self.run_output(&list_targets_args())?))
    }
}

// ---------- pure argv builders ----------

fn dir_args(dir: &Path) -> Vec<String> {
    vec!["-C".to_string(), dir.display().to_string()]
}

pub fn build_args(dir: &Path) -> Vec<String> {
    let mut a = dir_args(dir);
    a.push("build".to_string());
    a
}

/// Flash the project to `port`.
///
/// `flash` carries a dependency on the build, so this cannot send a binary
/// that did not just compile — see the module note.
pub fn flash_args(dir: &Path, port: &str) -> Vec<String> {
    let mut a = dir_args(dir);
    a.push("-p".to_string());
    a.push(port.to_string());
    a.push("flash".to_string());
    a
}

/// **Destructive.** Deletes `build/` and regenerates `sdkconfig`, discarding
/// hand-edited configuration. Never call this implicitly.
pub fn set_target_args(dir: &Path, target: &str) -> Vec<String> {
    let mut a = dir_args(dir);
    a.push("set-target".to_string());
    a.push(target.to_string());
    a
}

pub fn clean_args(dir: &Path, full: bool) -> Vec<String> {
    let mut a = dir_args(dir);
    a.push(if full { "fullclean" } else { "clean" }.to_string());
    a
}

pub fn list_targets_args() -> Vec<String> {
    vec!["--list-targets".to_string()]
}

// ---------- pure parsers ----------

/// The chip a project is configured for, from its `sdkconfig`.
///
/// The trap this exists to avoid: `sdkconfig` contains both
/// `CONFIG_IDF_TARGET="esp32s3"` and `CONFIG_IDF_TARGET_ESP32S3=y`, so
/// anything matching on a prefix finds the wrong line and reports a target of
/// `y`. Only the exact key counts.
pub fn parse_sdkconfig_target(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "CONFIG_IDF_TARGET" {
            continue;
        }
        let v = value.trim().trim_matches('"').trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    None
}

/// Where a project's console output goes.
///
/// This is the ESP-IDF counterpart of the Arduino `CDCOnBoot` trap: a board
/// that flashes perfectly and then prints nothing, because the one port you
/// can open is not the one the firmware is talking on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IdfConsoleChannel {
    /// A UART peripheral — on a dev board, the USB-to-UART bridge chip, which
    /// is a *different* serial port from the chip's own USB.
    Uart,
    /// USB CDC through the chip's USB OTG peripheral (ESP32-S2/S3).
    UsbCdc,
    /// The chip's built-in USB Serial/JTAG controller.
    UsbSerialJtag,
    /// Console compiled out entirely.
    None,
}

/// A project's console configuration, as `sdkconfig` describes it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IdfConsole {
    pub channel: IdfConsoleChannel,
    /// Whether ESP-IDF *also* mirrors output to USB Serial/JTAG.
    ///
    /// This is the field that decides whether a UART console is actually
    /// silent, and it is the default on every SoC with a USB Serial/JTAG
    /// controller. Its own Kconfig help says it exists for exactly the case
    /// in question — output "when UART0 port as a primary is selected but not
    /// connected". Ignoring it would fire a warning on ESP-IDF's stock
    /// configuration, which is the worst possible false positive.
    pub secondary_usb: bool,
    /// The console baud rate. `None` for USB channels, which have no baud —
    /// and where `sdkconfig` genuinely omits the key.
    pub baudrate: Option<u32>,
}

/// Read the console configuration out of an `sdkconfig`.
///
/// `None` when the file names no console channel at all, which is the honest
/// answer for a project that has never been configured: guessing ESP-IDF's
/// default here would mean warning about a setup we have not actually read.
///
/// Two exact-key traps, both real and both tested:
///
/// - `CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG_ENABLED` is a **different key** from
///   `CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG`, and is `y` even when the console is
///   USB CDC. Matching on a prefix reports the wrong channel.
/// - `CONFIG_ESP_CONSOLE_UART` is absent entirely when a USB channel is
///   selected, so the UART case must be checked last rather than first.
pub fn parse_sdkconfig_console(text: &str) -> Option<IdfConsole> {
    let mut on = std::collections::BTreeSet::new();
    let mut baudrate = None;
    for line in text.lines() {
        let line = line.trim();
        // `# CONFIG_X is not set` is how sdkconfig spells "off"; only an
        // explicit `=y` counts.
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if value == "y" {
            on.insert(key.to_string());
        } else if key == "CONFIG_ESP_CONSOLE_UART_BAUDRATE" {
            baudrate = value.parse::<u32>().ok();
        }
    }

    let has = |k: &str| on.contains(k);
    let channel = if has("CONFIG_ESP_CONSOLE_USB_CDC") {
        IdfConsoleChannel::UsbCdc
    } else if has("CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG") {
        IdfConsoleChannel::UsbSerialJtag
    } else if has("CONFIG_ESP_CONSOLE_NONE") {
        IdfConsoleChannel::None
    } else if has("CONFIG_ESP_CONSOLE_UART")
        || has("CONFIG_ESP_CONSOLE_UART_DEFAULT")
        || has("CONFIG_ESP_CONSOLE_UART_CUSTOM")
    {
        IdfConsoleChannel::Uart
    } else {
        return None;
    };

    Some(IdfConsole {
        channel,
        secondary_usb: has("CONFIG_ESP_CONSOLE_SECONDARY_USB_SERIAL_JTAG"),
        // Only meaningful for a UART console; the key is absent otherwise.
        baudrate: match channel {
            IdfConsoleChannel::Uart => baudrate,
            _ => None,
        },
    })
}

/// The target list `idf.py --list-targets` prints — one bare chip name per
/// line. Anything with whitespace or punctuation is a banner or a warning, not
/// a target.
pub fn parse_list_targets(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && l.starts_with("esp")
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
        .map(str::to_string)
        .collect()
}

/// A ninja progress line, e.g. `[412/1180] Building C object ...`.
fn is_progress(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('[') else {
        return false;
    };
    let Some((inside, _)) = rest.split_once(']') else {
        return false;
    };
    match inside.split_once('/') {
        Some((a, b)) => {
            !a.is_empty()
                && !b.is_empty()
                && a.chars().all(|c| c.is_ascii_digit())
                && b.chars().all(|c| c.is_ascii_digit())
        }
        None => false,
    }
}

/// The start of something the model can act on.
fn is_failure_marker(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("FAILED:")
        || t.starts_with("CMake Error")
        || t.contains("error:")
        || t.starts_with("ninja: error:")
}

/// The full compiler invocation ninja echoes under `FAILED:` — one enormous
/// line of `-D` and `-I` flags. It is roughly eight times the size of the
/// diagnostic it precedes and almost never the thing that needs fixing, so it
/// is dropped to leave budget for the errors themselves.
fn is_compiler_invocation(line: &str) -> bool {
    line.len() > 400 && (line.contains(" -D") || line.contains(" -I") || line.contains(" -o "))
}

/// How many lines of plain tail to fall back on when a build fails with no
/// recognisable marker at all.
const FALLBACK_TAIL: usize = 40;

/// Pick the part of a failed ESP-IDF build's output that a model can act on.
///
/// Sending the tail — which is what works for `arduino-cli` — fails badly
/// here, and the shape of the failure is worth recording. On a real 573-line
/// failing build: 496 lines were ninja progress, the actual error was at line
/// 509, and **ninja kept compiling for 64 more lines afterwards** before
/// stopping, so the last 20 lines were CMake bootloader configuration and the
/// error was nowhere near them. A 200-line tail came to 26 KB and did not
/// contain the diagnostic at all.
///
/// So instead of a tail, this takes each failure marker and the lines that
/// follow it up to the next progress line — the block ninja itself prints as a
/// unit. Multiple independent errors all survive, and the result for that same
/// build is about 300 bytes.
///
/// Falls back to a plain tail with progress lines removed when nothing matches,
/// because a build can fail in ways neither gcc nor CMake announces.
pub fn idf_failure_excerpt(lines: &[String]) -> Vec<String> {
    let mut keep = vec![false; lines.len()];
    let mut any = false;

    for (i, line) in lines.iter().enumerate() {
        if !is_failure_marker(line) {
            continue;
        }
        any = true;
        // The block runs from the marker until ninja starts reporting
        // progress again (or a new marker's block takes over).
        for (j, item) in keep.iter_mut().enumerate().skip(i) {
            if j > i && is_progress(&lines[j]) {
                break;
            }
            *item = true;
        }
    }

    if !any {
        let mut tail: Vec<String> = lines
            .iter()
            .filter(|l| !is_progress(l))
            .cloned()
            .collect();
        if tail.len() > FALLBACK_TAIL {
            tail.drain(..tail.len() - FALLBACK_TAIL);
        }
        return tail;
    }

    lines
        .iter()
        .zip(keep)
        .filter(|(line, k)| *k && !is_compiler_invocation(line))
        .map(|(line, _)| line.clone())
        .collect()
}

// ---------- tests ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn proj() -> PathBuf {
        PathBuf::from("/home/u/projects/blink")
    }

    #[test]
    fn build_names_the_project_directory() {
        assert_eq!(
            build_args(&proj()),
            vec!["-C", "/home/u/projects/blink", "build"]
        );
    }

    #[test]
    fn flash_carries_the_port_and_needs_no_explicit_build() {
        // `flash` depends on `all` inside idf.py, so spelling "build flash"
        // here would be redundant rather than protective.
        assert_eq!(
            flash_args(&proj(), "/dev/ttyACM0"),
            vec!["-C", "/home/u/projects/blink", "-p", "/dev/ttyACM0", "flash"]
        );
    }

    #[test]
    fn nothing_routine_can_reach_set_target() {
        // set-target destroys sdkconfig, so it must never appear in the argv
        // of an ordinary build or flash.
        for args in [build_args(&proj()), flash_args(&proj(), "/dev/ttyACM0")] {
            assert!(
                !args.iter().any(|a| a == "set-target"),
                "set-target leaked into {args:?}"
            );
        }
    }

    #[test]
    fn set_target_names_the_chip() {
        assert_eq!(
            set_target_args(&proj(), "esp32c6"),
            vec!["-C", "/home/u/projects/blink", "set-target", "esp32c6"]
        );
    }

    #[test]
    fn clean_and_fullclean_are_different_actions() {
        assert!(clean_args(&proj(), false).contains(&"clean".to_string()));
        assert!(clean_args(&proj(), true).contains(&"fullclean".to_string()));
    }

    // ---------- sdkconfig ----------

    #[test]
    fn the_target_comes_from_the_exact_key() {
        let cfg = "\
CONFIG_IDF_TARGET_ESP32S3=y
CONFIG_IDF_TARGET=\"esp32s3\"
CONFIG_IDF_TARGET_ARCH_XTENSA=y
";
        assert_eq!(parse_sdkconfig_target(cfg).as_deref(), Some("esp32s3"));
    }

    #[test]
    fn a_prefix_match_alone_is_not_a_target() {
        // The trap: matching on a prefix finds this line and reports "y".
        let cfg = "CONFIG_IDF_TARGET_ESP32S3=y\nCONFIG_IDF_TARGET_ARCH_XTENSA=y\n";
        assert_eq!(parse_sdkconfig_target(cfg), None);
    }

    #[test]
    fn a_project_with_no_target_yet_reports_none() {
        assert_eq!(parse_sdkconfig_target(""), None);
        assert_eq!(parse_sdkconfig_target("CONFIG_IDF_TARGET=\"\"\n"), None);
    }

    // ---------- target list ----------

    #[test]
    fn the_target_list_keeps_only_chip_names() {
        let out = "\
Running idf_size.py...
esp32
esp32s2
esp32c3
esp32s3
esp32c6
esp32p4

WARNING: something
";
        assert_eq!(
            parse_list_targets(out),
            vec!["esp32", "esp32s2", "esp32c3", "esp32s3", "esp32c6", "esp32p4"]
        );
    }

    // ---------- failure excerpt ----------

    fn failure_log() -> Vec<String> {
        include_str!("testdata/idf_build_failure.log")
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn the_excerpt_contains_the_diagnostic_a_tail_would_miss() {
        let out = idf_failure_excerpt(&failure_log());
        let joined = out.join("\n");
        assert!(
            joined.contains("error: implicit declaration of function 'undefined_function_here'"),
            "the actual error must survive, got:\n{joined}"
        );
        assert!(joined.contains("FAILED:"));
    }

    #[test]
    fn ninja_progress_lines_do_not_reach_the_model() {
        let out = idf_failure_excerpt(&failure_log());
        assert!(
            !out.iter().any(|l| is_progress(l)),
            "progress lines leaked: {out:?}"
        );
    }

    #[test]
    fn the_bootloader_cmake_noise_after_the_failure_is_dropped() {
        // ninja keeps going after the error, so everything below it is
        // unrelated -- this is exactly what a tail would have sent instead.
        let out = idf_failure_excerpt(&failure_log()).join("\n");
        assert!(!out.contains("Build files have been written to"));
        assert!(!out.contains("Found Git:"));
    }

    #[test]
    fn the_compiler_invocation_is_dropped() {
        let out = idf_failure_excerpt(&failure_log());
        assert!(
            !out.iter().any(|l| l.contains("riscv32-esp-elf-gcc") && l.len() > 400),
            "the flag dump should not survive"
        );
    }

    #[test]
    fn the_excerpt_is_small_enough_to_leave_room_for_more_errors() {
        let bytes: usize = idf_failure_excerpt(&failure_log())
            .iter()
            .map(|l| l.len() + 1)
            .sum();
        assert!(bytes < 2_000, "excerpt was {bytes} bytes");
    }

    #[test]
    fn two_independent_errors_both_survive() {
        let lines: Vec<String> = [
            "[1/9] Building a.c.obj",
            "FAILED: a.c.obj",
            "a.c:1:1: error: first problem",
            "[2/9] Building b.c.obj",
            "FAILED: b.c.obj",
            "b.c:2:2: error: second problem",
            "[3/9] Linking",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let out = idf_failure_excerpt(&lines).join("\n");
        assert!(out.contains("first problem"), "{out}");
        assert!(out.contains("second problem"), "{out}");
    }

    #[test]
    fn a_cmake_error_is_a_marker_even_though_gcc_never_ran() {
        let lines: Vec<String> = [
            "[1/9] Building",
            "CMake Error at CMakeLists.txt:4 (idf_component_register):",
            "  Unknown component 'nonexistent'",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let out = idf_failure_excerpt(&lines).join("\n");
        assert!(out.contains("Unknown component"), "{out}");
    }

    #[test]
    fn a_failure_with_no_marker_falls_back_to_a_tail_without_progress() {
        // A build can die in ways neither gcc nor CMake announces; returning
        // nothing at all would be worse than returning the end of the log.
        let mut lines: Vec<String> =
            (1..=60).map(|i| format!("[{i}/60] Building thing{i}")).collect();
        lines.push("something went wrong in a way we do not parse".to_string());
        let out = idf_failure_excerpt(&lines);
        assert_eq!(out, vec!["something went wrong in a way we do not parse"]);
    }

    // ---------- console channel ----------
    //
    // Every block below is a verbatim `CONFIG_ESP_CONSOLE*` section from a real
    // sdkconfig generated by ESP-IDF v6.0.1, not a hand-written approximation.

    /// esp32c3, stock. Primary UART — but the secondary mirrors to USB.
    const C3_STOCK: &str = "\
CONFIG_ESP_CONSOLE_UART_DEFAULT=y
CONFIG_ESP_CONSOLE_SECONDARY_USB_SERIAL_JTAG=y
CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG_ENABLED=y
CONFIG_ESP_CONSOLE_UART=y
CONFIG_ESP_CONSOLE_UART_NUM=0
CONFIG_ESP_CONSOLE_UART_BAUDRATE=115200
";

    /// esp32c3, USB Serial/JTAG chosen as the primary console.
    const C3_JTAG: &str = "\
CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG=y
CONFIG_ESP_CONSOLE_SECONDARY_NONE=y
CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG_ENABLED=y
CONFIG_ESP_CONSOLE_UART_NUM=-1
";

    /// esp32s3, USB CDC primary.
    const S3_CDC: &str = "\
CONFIG_ESP_CONSOLE_USB_CDC=y
CONFIG_ESP_CONSOLE_SECONDARY_USB_SERIAL_JTAG=y
CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG_ENABLED=y
CONFIG_ESP_CONSOLE_UART_NUM=-1
CONFIG_ESP_CONSOLE_USB_CDC_RX_BUF_SIZE=64
";

    /// esp32s3, UART primary with the secondary switched off — the one
    /// configuration that is actually silent on a native-USB port.
    const S3_SILENT: &str = "\
CONFIG_ESP_CONSOLE_UART_DEFAULT=y
CONFIG_ESP_CONSOLE_SECONDARY_NONE=y
CONFIG_ESP_CONSOLE_UART=y
CONFIG_ESP_CONSOLE_UART_NUM=0
CONFIG_ESP_CONSOLE_UART_BAUDRATE=115200
";

    #[test]
    fn the_stock_uart_console_still_reaches_usb_via_the_secondary() {
        // ESP-IDF's default on any SoC with USB Serial/JTAG. Treating this as
        // silent would warn on the stock configuration.
        let c = parse_sdkconfig_console(C3_STOCK).unwrap();
        assert_eq!(c.channel, IdfConsoleChannel::Uart);
        assert!(c.secondary_usb);
        assert_eq!(c.baudrate, Some(115200));
    }

    #[test]
    fn a_usb_serial_jtag_console_is_not_confused_with_the_enabled_flag() {
        // CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG_ENABLED is `y` in three of these
        // four fixtures and means something else entirely. Only the exact key
        // selects the channel.
        let c = parse_sdkconfig_console(C3_JTAG).unwrap();
        assert_eq!(c.channel, IdfConsoleChannel::UsbSerialJtag);
        assert!(!c.secondary_usb);
        // No UART, so no baud — and sdkconfig really does omit the key.
        assert_eq!(c.baudrate, None);
    }

    #[test]
    fn usb_cdc_wins_even_though_the_jtag_enabled_flag_is_also_set() {
        let c = parse_sdkconfig_console(S3_CDC).unwrap();
        assert_eq!(c.channel, IdfConsoleChannel::UsbCdc);
        assert_eq!(c.baudrate, None);
    }

    #[test]
    fn uart_with_no_secondary_is_the_silent_configuration() {
        let c = parse_sdkconfig_console(S3_SILENT).unwrap();
        assert_eq!(c.channel, IdfConsoleChannel::Uart);
        assert!(!c.secondary_usb);
        assert_eq!(c.baudrate, Some(115200));
    }

    #[test]
    fn a_console_disabled_entirely_is_reported_as_such() {
        let c = parse_sdkconfig_console(
            "CONFIG_ESP_CONSOLE_NONE=y\nCONFIG_ESP_CONSOLE_UART_NONE=y\n",
        )
        .unwrap();
        assert_eq!(c.channel, IdfConsoleChannel::None);
    }

    #[test]
    fn an_sdkconfig_naming_no_channel_reports_nothing_rather_than_guessing() {
        // Guessing ESP-IDF's default here would mean warning about a
        // configuration we never actually read.
        assert!(parse_sdkconfig_console("CONFIG_IDF_TARGET=\"esp32c3\"\n").is_none());
        assert!(parse_sdkconfig_console("").is_none());
    }

    #[test]
    fn an_option_switched_off_is_not_read_as_on() {
        // sdkconfig spells "off" as a comment, which must not become a key.
        let off = "# CONFIG_ESP_CONSOLE_USB_CDC is not set\nCONFIG_ESP_CONSOLE_UART=y\n";
        assert_eq!(
            parse_sdkconfig_console(off).unwrap().channel,
            IdfConsoleChannel::Uart
        );
    }
}
