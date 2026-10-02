use clap::{Parser, Subcommand, ValueEnum};
use em_preview::{server, state::PreviewState, watcher};
use em_script::Scene;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "enclosure-maker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum StlFormat {
    Binary,
    Ascii,
}

#[derive(Subcommand)]
enum Command {
    /// Watch a .rhai script and serve a live-updating 3D preview in the browser.
    Preview {
        #[arg(long)]
        script: PathBuf,
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Preview just this one named part instead of the whole assembly.
        #[arg(long)]
        part: Option<String>,
    },
    /// Evaluate a .rhai script once and write the result to a file.
    ///
    /// `--output FILE.stl` exports a single part (`--part NAME`, or
    /// automatically if the script only defines one). `--output FILE.3mf`
    /// bundles multiple parts into one file. `--output-dir DIR` writes each
    /// part to `DIR/<name>.stl`. Batch modes (.3mf / --output-dir) need an
    /// explicit `--parts a,b,c` list whenever the script defines more than
    /// one part, since a script's parts often include overlapping "view"
    /// compositions (assembly/exploded/section) alongside printable ones.
    Export {
        #[arg(long)]
        script: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        output_dir: Option<PathBuf>,
        #[arg(long)]
        part: Option<String>,
        #[arg(long)]
        parts: Option<String>,
        #[arg(long, value_enum, default_value_t = StlFormat::Binary)]
        format: StlFormat,
        /// Override a script param(...) value, e.g. --param Width=80.0. Repeatable.
        #[arg(long = "param")]
        params: Vec<String>,
    },
}

/// The `enclosure-maker --agent-guard <project_dir>` entry point: the
/// `PreToolUse` hook an embedded assistant session is started with. Reads
/// the CLI's hook JSON from stdin, adjudicates it with
/// `em_agent::guard_decision`, and prints a deny payload (or nothing) on
/// stdout. Always exits 0 -- a non-zero exit is how a hook reports its own
/// failure, which `claude` handles as "log and continue" (fail open, which
/// this must never do); the refusal is carried by the printed JSON instead.
fn run_agent_guard(project_dir: &str) {
    let mut stdin_body = String::new();
    // A read error yields an empty body, which `guard_decision` denies --
    // failing closed, same as unparseable input.
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin_body);
    if let Some(deny) = em_agent::guard_decision(std::path::Path::new(project_dir), &stdin_body) {
        println!("{deny}");
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Checked before clap's normal parsing: this is a hidden entry point the
    // `claude` CLI itself invokes as a hook command, not a user-facing
    // subcommand.
    let raw_args: Vec<String> = std::env::args().collect();
    if let Some(idx) = raw_args.iter().position(|a| a == "--agent-guard") {
        if let Some(dir) = raw_args.get(idx + 1) {
            run_agent_guard(dir);
        }
        return Ok(());
    }

    let cli = Cli::parse();
    match cli.command {
        Command::Preview { script, port, part } => run_preview(script, port, part).await,
        Command::Export {
            script,
            output,
            output_dir,
            part,
            parts,
            format,
            params,
        } => run_export(script, output, output_dir, part, parts, format, params),
    }
}

fn parse_param_overrides(params: &[String]) -> anyhow::Result<HashMap<String, f64>> {
    let mut overrides = HashMap::new();
    for entry in params {
        let (name, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--param must be NAME=VALUE, got '{entry}'"))?;
        let value: f64 = value
            .parse()
            .map_err(|_| anyhow::anyhow!("--param '{entry}': '{value}' is not a number"))?;
        overrides.insert(name.to_string(), value);
    }
    Ok(overrides)
}

fn load_scene(script: &PathBuf, overrides: &HashMap<String, f64>) -> anyhow::Result<Scene> {
    let script_dir = script.parent().unwrap_or_else(|| std::path::Path::new("."));
    let engine = em_script::ScriptEngine::with_import_root(script_dir);
    let (scene, _params) = engine.eval_file_with_params(script, overrides)?;
    Ok(scene)
}

/// Resolves which single part a `.stl`/`--part`-style export should use:
/// the explicit `--part`, or the script's only part if it has just one.
fn resolve_single_part(scene: &Scene, part_arg: Option<&str>) -> anyhow::Result<String> {
    if let Some(name) = part_arg {
        if scene.part(name).is_none() {
            anyhow::bail!(
                "no part named '{name}' (available: {})",
                scene.part_names().collect::<Vec<_>>().join(", ")
            );
        }
        return Ok(name.to_string());
    }
    let names: Vec<&str> = scene.part_names().collect();
    match names.as_slice() {
        [only] => Ok(only.to_string()),
        _ => anyhow::bail!(
            "script defines {} parts ({}) -- pass --part NAME to pick one",
            names.len(),
            names.join(", ")
        ),
    }
}

/// Resolves which parts a batch export (`--output-dir` or a `.3mf` output)
/// should include: the explicit comma-separated `--parts` list, or the
/// script's only part if it has just one.
fn resolve_batch_parts(scene: &Scene, parts_arg: Option<&str>) -> anyhow::Result<Vec<String>> {
    if let Some(list) = parts_arg {
        let names: Vec<String> = list.split(',').map(|s| s.trim().to_string()).collect();
        for name in &names {
            if scene.part(name).is_none() {
                anyhow::bail!(
                    "no part named '{name}' (available: {})",
                    scene.part_names().collect::<Vec<_>>().join(", ")
                );
            }
        }
        return Ok(names);
    }
    let names: Vec<&str> = scene.part_names().collect();
    match names.as_slice() {
        [only] => Ok(vec![only.to_string()]),
        _ => anyhow::bail!(
            "script defines {} parts ({}) -- pass --parts a,b,c to pick which ones to export together",
            names.len(),
            names.join(", ")
        ),
    }
}

fn run_export(
    script: PathBuf,
    output: Option<PathBuf>,
    output_dir: Option<PathBuf>,
    part: Option<String>,
    parts: Option<String>,
    format: StlFormat,
    param_args: Vec<String>,
) -> anyhow::Result<()> {
    let overrides = parse_param_overrides(&param_args)?;
    let scene = load_scene(&script, &overrides)?;
    let transforms = em_preview::transforms::load(&script).map_err(anyhow::Error::msg)?;
    let transformed = |name: &str| {
        transforms
            .get(name)
            .cloned()
            .unwrap_or_default()
            .apply(scene.part(name).unwrap())
    };

    match (output, output_dir) {
        (Some(_), Some(_)) => anyhow::bail!("pass either --output or --output-dir, not both"),
        (None, None) => anyhow::bail!("pass either --output FILE or --output-dir DIR"),

        (Some(output), None) => {
            if output.extension().and_then(|e| e.to_str()) == Some("3mf") {
                let names = resolve_batch_parts(&scene, parts.as_deref())?;
                let selected: Vec<(String, em_core::Mesh)> = names
                    .into_iter()
                    .map(|n| (n.clone(), transformed(&n)))
                    .collect();
                em_export::write_3mf(&selected, &output)?;
                println!("wrote {} part(s) to {}", selected.len(), output.display());
            } else {
                let name = resolve_single_part(&scene, part.as_deref())?;
                let mesh = transformed(&name);
                match format {
                    StlFormat::Binary => em_export::write_binary_stl(&mesh, &output)?,
                    StlFormat::Ascii => em_export::write_ascii_stl(&mesh, &name, &output)?,
                }
                println!(
                    "wrote part '{name}' ({} triangles) to {}",
                    mesh.triangle_count(),
                    output.display()
                );
            }
        }

        (None, Some(dir)) => {
            std::fs::create_dir_all(&dir)?;
            let names = resolve_batch_parts(&scene, parts.as_deref())?;
            for name in &names {
                let mesh = transformed(name);
                let file = dir.join(format!("{name}.stl"));
                match format {
                    StlFormat::Binary => em_export::write_binary_stl(&mesh, &file)?,
                    StlFormat::Ascii => em_export::write_ascii_stl(&mesh, name, &file)?,
                }
                println!(
                    "wrote part '{name}' ({} triangles) to {}",
                    mesh.triangle_count(),
                    file.display()
                );
            }
        }
    }

    Ok(())
}

async fn run_preview(script: PathBuf, port: u16, part: Option<String>) -> anyhow::Result<()> {
    let (state, rx) = PreviewState::new(script, part);
    let _watcher = watcher::spawn_watcher(state.clone())?;
    server::serve(port, state, rx).await
}
