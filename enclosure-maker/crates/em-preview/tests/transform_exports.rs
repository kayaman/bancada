use std::{path::Path, process::Command};

fn export(script: &Path, args: &[&str]) {
    let output = Command::new(env!("CARGO_BIN_EXE_enclosure-maker"))
        .args(["export", "--script"])
        .arg(script)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn x_bounds(path: &Path) -> (f32, f32) {
    let bytes = std::fs::read(path).unwrap();
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for triangle in bytes[84..].chunks_exact(50) {
        for vertex in 0..3 {
            let offset = 12 + vertex * 12;
            let x = f32::from_le_bytes(triangle[offset..offset + 4].try_into().unwrap());
            min = min.min(x);
            max = max.max(x);
        }
    }
    (min, max)
}

#[test]
fn cli_single_and_batch_stl_use_saved_part_transforms() {
    let dir = std::env::temp_dir().join(format!("bancada-transform-cli-{}", std::process::id()));
    std::fs::create_dir_all(dir.join(".enclosure-maker")).unwrap();
    let script = dir.join("main.rhai");
    std::fs::write(
        &script,
        "emit(\"base\", cuboid(10.0, 20.0, 6.0)); emit(\"lid\", cuboid(4.0, 4.0, 2.0));",
    )
    .unwrap();
    std::fs::write(
        dir.join(".enclosure-maker/main.rhai.transforms.json"),
        r#"{"base":{"translation":[3.25,0,0],"rotation":[0,0,90]}}"#,
    )
    .unwrap();

    let single = dir.join("single.stl");
    export(
        &script,
        &["--part", "base", "--output", single.to_str().unwrap()],
    );
    assert_eq!(x_bounds(&single), (-6.75, 13.25));

    let batch = dir.join("batch");
    export(
        &script,
        &[
            "--parts",
            "base,lid",
            "--output-dir",
            batch.to_str().unwrap(),
        ],
    );
    assert_eq!(
        std::fs::read(single).unwrap(),
        std::fs::read(batch.join("base.stl")).unwrap()
    );
    assert_eq!(x_bounds(&batch.join("lid.stl")), (-2.0, 2.0));
    std::fs::remove_dir_all(dir).unwrap();
}
