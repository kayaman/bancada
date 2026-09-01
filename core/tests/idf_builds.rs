//! A real ESP-IDF build, end to end, through `IdfCli`.
//!
//! Opt-in with `BANCADA_IDF_LIVE=1`. This is the only test that proves the
//! environment map [`idfenv::activate`] produces is actually *sufficient* —
//! every other test proves it is well-formed, which is not the same thing.
//!
//!   BANCADA_IDF_LIVE=1 cargo test -p bancada-core --test idf_builds -- --nocapture

use std::path::PathBuf;

use bancada_core::{idf, idfenv};

fn enabled() -> bool {
    std::env::var("BANCADA_IDF_LIVE").as_deref() == Ok("1")
}

fn resolve() -> (idf::IdfCli, PathBuf) {
    let reg_path = std::env::var_os("BANCADA_IDF_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var("HOME").unwrap()).join(".espressif/tools/eim_idf.json")
        });
    let json = std::fs::read_to_string(&reg_path).expect("read registry");
    let reg = idfenv::parse_registry(&json, &reg_path).expect("parse registry");
    let prefer = std::env::var("BANCADA_IDF_VERSION").ok();
    let install = idfenv::select_install(&reg, prefer.as_deref()).expect("select");
    let env = idfenv::activate(install, &std::env::var("PATH").unwrap_or_default())
        .expect("activate");
    let idf_path = install.path.clone();
    (
        idf::IdfCli::new(install.python.clone(), idf_path.clone(), env),
        idf_path,
    )
}

#[test]
fn hello_world_sets_a_target_and_builds() {
    if !enabled() {
        eprintln!("skipped: set BANCADA_IDF_LIVE=1 to run");
        return;
    }
    let (cli, idf_path) = resolve();

    let targets = cli.list_targets().expect("list targets");
    assert!(targets.contains(&"esp32c3".to_string()), "got {targets:?}");

    // Copy the vendor example somewhere writable; never build in $IDF_PATH.
    let work = std::env::temp_dir().join(format!("bancada-idf-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    let src = idf_path.join("examples/get-started/hello_world");
    copy_dir(&src, &work);

    let mut log = Vec::new();
    let r = cli
        .set_target(&work, "esp32c3", |l| log.push(l.line))
        .expect("set-target ran");
    assert!(r.success, "set-target failed:\n{}", log.join("\n"));

    log.clear();
    let r = cli.build(&work, |l| log.push(l.line)).expect("build ran");
    assert!(r.success, "build failed:\n{}", log.join("\n"));

    let bin = work.join("build/hello_world.bin");
    assert!(bin.exists(), "no binary at {}", bin.display());
    eprintln!("built {} bytes", std::fs::metadata(&bin).unwrap().len());

    let _ = std::fs::remove_dir_all(&work);
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

#[test]
fn a_broken_build_yields_an_excerpt_the_agent_can_act_on() {
    if !enabled() {
        eprintln!("skipped: set BANCADA_IDF_LIVE=1 to run");
        return;
    }
    let (cli, idf_path) = resolve();
    let work = std::env::temp_dir().join(format!("bancada-idf-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    copy_dir(&idf_path.join("examples/get-started/hello_world"), &work);

    let mut log = Vec::new();
    assert!(cli
        .set_target(&work, "esp32c3", |l| log.push(l.line))
        .expect("set-target ran")
        .success);

    // Introduce an ordinary compile error.
    let main_c = work.join("main/hello_world_main.c");
    let src = std::fs::read_to_string(&main_c).unwrap();
    std::fs::write(
        &main_c,
        src.replace(
            "void app_main(void)\n{",
            "void app_main(void)\n{\n    deliberately_undefined_symbol();",
        ),
    )
    .unwrap();

    log.clear();
    let r = cli.build(&work, |l| log.push(l.line)).expect("build ran");
    assert!(!r.success, "the build was supposed to fail");

    let excerpt = bancada_core::idf::idf_failure_excerpt(&log);
    let text = excerpt.join("\n");
    let bytes: usize = text.len();
    eprintln!(
        "full log {} lines / {} bytes -> excerpt {} lines / {bytes} bytes",
        log.len(),
        log.iter().map(|l| l.len() + 1).sum::<usize>(),
        excerpt.len()
    );

    assert!(
        text.contains("deliberately_undefined_symbol"),
        "the excerpt must name the offending symbol:\n{text}"
    );
    assert!(
        !excerpt.iter().any(|l| l.starts_with("[") && l.contains("] Building")),
        "ninja progress leaked into the excerpt"
    );
    assert!(bytes < 8_000, "excerpt was {bytes} bytes; too noisy to be useful");

    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_real_sdkconfig_reports_its_console_channel() {
    if !enabled() {
        eprintln!("skipped: set BANCADA_IDF_LIVE=1 to run");
        return;
    }
    use bancada_core::idf::{parse_sdkconfig_console, IdfConsoleChannel};

    let (cli, idf_path) = resolve();
    let work = std::env::temp_dir().join(format!("bancada-idf-console-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    copy_dir(&idf_path.join("examples/get-started/hello_world"), &work);

    let read = |w: &PathBuf| std::fs::read_to_string(w.join("sdkconfig")).expect("sdkconfig");
    let mut log = Vec::new();

    // 1. Stock. ESP-IDF defaults to a UART primary *with* the USB Serial/JTAG
    //    secondary, which is precisely why a UART console is not evidence of
    //    silence on its own.
    assert!(cli
        .set_target(&work, "esp32c3", |l| log.push(l.line))
        .expect("set-target")
        .success);
    let stock = parse_sdkconfig_console(&read(&work)).expect("stock console");
    assert_eq!(stock.channel, IdfConsoleChannel::Uart);
    assert!(
        stock.secondary_usb,
        "ESP-IDF's own default must be read as mirrored, or the warning fires on every project"
    );
    assert_eq!(stock.baudrate, Some(115200));

    // 2. USB Serial/JTAG as the primary. Note sdkconfig omits the UART and
    //    baudrate keys entirely here — the parser must not infer them.
    std::fs::write(
        work.join("sdkconfig.defaults"),
        "CONFIG_ESP_CONSOLE_USB_SERIAL_JTAG=y\n",
    )
    .unwrap();
    std::fs::remove_file(work.join("sdkconfig")).unwrap();
    log.clear();
    assert!(cli
        .set_target(&work, "esp32c3", |l| log.push(l.line))
        .expect("set-target")
        .success);
    let jtag = parse_sdkconfig_console(&read(&work)).expect("jtag console");
    assert_eq!(jtag.channel, IdfConsoleChannel::UsbSerialJtag);
    assert_eq!(jtag.baudrate, None);

    let _ = std::fs::remove_dir_all(&work);
}
