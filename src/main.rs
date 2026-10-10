use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use flashmob::exec::{self, ExecResponse};
use flashmob::model::Project;
use flashmob::study::{self, Studies};

#[cfg(feature = "gui")]
mod ui;

#[derive(Parser)]
#[command(
    name = "flashmob",
    version,
    about = "Load flow, short circuit, coordination, and IEEE 1584-2018 arc flash",
    long_about = "Flashmob is a power-system study tool.\n\n\
The window is `flashmob gui`.\n\n\
For a model or any other headless caller, stdout of schema, sample, validate, topology, run, exec, sld, and tcc is data and there are no prompts. \
`flashmob schema` describes the JSON project and the exec protocol. \
`flashmob sld` writes a single-line diagram with short-circuit and arc-flash results on every bus. \
`flashmob tcc` writes a time-current curve."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Open the macOS study window.
    Gui {
        /// Project JSON to open. Omit to start with the built-in sample. If the file
        /// cannot be read, the window starts with no project and shows the error.
        project: Option<PathBuf>,
    },
    /// Print the JSON contract: project shape, exec commands, and a sample.
    Schema,
    /// Print the built-in sample project as JSON.
    Sample {
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        compact: bool,
    },
    /// Check a project file. Prints JSON.
    Validate {
        path: PathBuf,
    },
    /// Print entered bus, branch, and component connections as JSON. No study is run.
    Topology {
        /// Project JSON file. Use - to read stdin.
        path: PathBuf,
        #[arg(long)]
        compact: bool,
    },
    /// Run studies. Prints JSON, or a text report with --text.
    Run {
        /// Project JSON file. Use - to read stdin.
        path: PathBuf,
        /// Study name, repeated. Default is all of loadflow, fault, arcflash, coordination.
        #[arg(long = "study")]
        study: Vec<String>,
        #[arg(long)]
        text: bool,
        #[arg(long)]
        compact: bool,
    },
    /// Apply a JSON command list from a file or stdin. Prints JSON.
    Exec {
        /// Request JSON file. Omit to read stdin.
        file: Option<PathBuf>,
        #[arg(long)]
        compact: bool,
    },
    /// Write an SVG single-line diagram with fault and arc-flash results on each bus.
    Sld {
        path: PathBuf,
        /// SVG path, or - for stdout.
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Write a time-current curve as SVG, and optionally CSV.
    Tcc {
        path: PathBuf,
        /// SVG path, or - for stdout.
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        csv: Option<PathBuf>,
        /// Voltage the curves are referred to. Defaults to the lowest device voltage.
        #[arg(long)]
        ref_kv: Option<f64>,
        /// Device id or name. Repeat to keep a subset.
        #[arg(long = "device")]
        device: Vec<String>,
    },
    /// Write an engineering data package for mapping (third-party import unverified).
    /// The project must validate. Existing files are never replaced without --force,
    /// and the input project file is never replaced.
    ExportPackage {
        path: PathBuf,
        /// Folder for the export, or an .xml path: project.xml is written there and the
        /// other files beside it. Other file-like paths (.json, .csv, ...) are rejected.
        #[arg(short, long)]
        output: PathBuf,
        /// Replace existing package files in the output.
        #[arg(long)]
        force: bool,
    },
    /// Convert a binary manufacturer device library into a Flashmob library JSON file.
    /// Prints a JSON summary. Existing output is never replaced without --force.
    ImportLibrary {
        /// Binary manufacturer device library file.
        input: PathBuf,
        /// Library JSON to write.
        #[arg(short, long)]
        output: PathBuf,
        #[arg(long)]
        compact: bool,
        #[arg(long)]
        force: bool,
        /// Term to remove from the output (repeatable, case-insensitive). Free text has it
        /// replaced by [redacted]; a device whose identifying text contains it is withheld.
        #[arg(long = "redact")]
        redact: Vec<String>,
    },
    /// Search a Flashmob library JSON file. Prints matching devices as JSON.
    Library {
        path: PathBuf,
        /// Terms that must all appear in manufacturer, category, type, style, catalog, or reference.
        terms: Vec<String>,
        /// Print at most this many devices.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Copy one library point curve onto a project device (a snapshot).
    /// Prints the device JSON, or the whole project when --project is set.
    ApplyLibrary {
        /// Flashmob device-library JSON.
        library: PathBuf,
        #[arg(long)]
        device_id: u32,
        #[arg(long)]
        id: String,
        #[arg(long)]
        bus: String,
        /// Empty builds a name from the manufacturer, style, catalog, and rating.
        #[arg(long, default_value = "")]
        name: String,
        /// Trip-unit rating when the library device has more than one.
        #[arg(long)]
        rating_a: Option<f64>,
        /// Ampere scale for a curve stored as multiples.
        #[arg(long)]
        current_base_a: Option<f64>,
        /// Recorded on the library link. Does not select a trip unit.
        #[arg(long)]
        voltage_v: Option<f64>,
        /// Point-curve name when the trip unit has more than one.
        #[arg(long)]
        function_name: Option<String>,
        #[arg(long)]
        protected_branch: Option<String>,
        /// Label stored on the library link. Empty stores the library path.
        #[arg(long, default_value = "")]
        library_name: String,
        /// The governing band already includes total clearing. Off unless set.
        #[arg(long, default_value_t = false)]
        fuse_total_clearing: bool,
        /// Project to add the device to. Omit to print the device only.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Output JSON path. Omit, or -, for stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Gui { project } => gui(project),
        Command::Schema => emit(&exec::schema(), false),
        Command::Sample { output, compact } => {
            let value = serde_json::to_value(Project::sample()).expect("sample serializes");
            write_json(output.as_deref(), &value, compact)
        }
        Command::Validate { path } => match read_project(&path) {
            Ok(project) => {
                let errors = project.validate();
                emit(&serde_json::json!({"ok": errors.is_empty(), "errors": errors}), false)
                    .max_code(if errors.is_empty() { ExitCode::SUCCESS } else { ExitCode::from(1) })
            }
            Err(err) => fail(&err),
        },
        Command::Topology { path, compact } => match read_project(&path)
            .and_then(|project| flashmob::connectivity::topology(&project)) {
            Ok(topology) => emit(&serde_json::json!({"ok": true, "topology": topology}), compact),
            Err(err) => fail(&err),
        },
        Command::Run { path, study, text, compact } => match read_project(&path).and_then(|p| Studies::parse(&study).map(|s| (p, s))) {
            Ok((project, studies)) => match study::run(&project, studies) {
                Ok(results) => {
                    if text {
                        println!("{}", study::text_report(&project, &results));
                        ExitCode::SUCCESS
                    } else {
                        let value = serde_json::json!({"ok": true, "project_name": project.name, "results": results});
                        emit(&value, compact)
                    }
                }
                Err(err) => fail(&err),
            },
            Err(err) => fail(&err),
        },
        Command::Exec { file, compact } => {
            let text = match file {
                Some(path) => match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(err) => return fail(&format!("could not read {}: {err}", path.display())),
                },
                None => {
                    let mut text = String::new();
                    if let Err(err) = std::io::stdin().read_to_string(&mut text) {
                        return fail(&format!("could not read stdin: {err}"));
                    }
                    text
                }
            };
            let response = exec::exec_request(&text);
            let code = if response.ok { ExitCode::SUCCESS } else { ExitCode::from(1) };
            emit_response(&response, compact, code)
        }
        Command::Sld { path, output } => match prepare(&path) {
            Ok((project, results)) => {
                let svg = flashmob::sld::to_svg(&flashmob::sld::diagram(&project, &results));
                write_text(output.as_path(), &svg)
            }
            Err(err) => fail(&err),
        },
        Command::Tcc { path, output, csv, ref_kv, device } => match prepare(&path) {
            Ok((project, results)) => match flashmob::tcc::plot(&project, Some(&results), ref_kv, &device) {
                Ok(plot) => {
                    if let Some(csv_path) = &csv {
                        if let Err(err) = std::fs::write(csv_path, flashmob::tcc::to_csv(&plot)) {
                            return fail(&format!("could not write {}: {err}", csv_path.display()));
                        }
                    }
                    write_text(output.as_path(), &flashmob::tcc::to_svg(&plot))
                }
                Err(err) => fail(&err),
            },
            Err(err) => fail(&err),
        },
        Command::ExportPackage { path, output, force } => match read_project(&path) {
            Ok(project) => {
                let errors = project.validate();
                if !errors.is_empty() {
                    return emit(&serde_json::json!({"ok": false, "error": "project is invalid; nothing exported", "errors": errors}), false)
                        .max_code(ExitCode::from(1));
                }
                let input = (path.as_os_str() != "-").then_some(path.as_path());
                let options = flashmob::package::WriteOptions { overwrite: force, protect: input };
                match flashmob::package::write_dir(&project, output.as_path(), options) {
                    Ok(dir) => {
                        println!("{}", serde_json::json!({"ok": true, "directory": dir.display().to_string()}));
                        ExitCode::SUCCESS
                    }
                    Err(err) => fail(&err),
                }
            }
            Err(err) => fail(&err),
        },
        Command::ImportLibrary { input, output, compact, force, redact } => import_library(&input, &output, compact, force, &redact),
        Command::Library { path, terms, limit } => {
            let library = match std::fs::read_to_string(&path).map_err(|err| format!("could not read {}: {err}", path.display()))
                .and_then(|text| flashmob::library::Library::from_json(&text)) {
                Ok(library) => library,
                Err(err) => return fail(&err),
            };
            let found = library.search(&terms);
            emit(&serde_json::json!({"ok": true, "matches": found.len(), "devices": found.into_iter().take(limit).collect::<Vec<_>>()}), false)
        }
        Command::ApplyLibrary { library, device_id, id, bus, name, rating_a, current_base_a, voltage_v, function_name, protected_branch, library_name, fuse_total_clearing, project, output } => {
            let library_path = library.display().to_string();
            let request = flashmob::library::ApplyDevice {
                device_id,
                id,
                name,
                bus,
                protected_branch,
                rating_a,
                current_base_a,
                voltage_v,
                function_name,
                library_name: if library_name.is_empty() { library_path.clone() } else { library_name },
                fuse_total_clearing,
            };
            match project {
                Some(project) => apply_library_project(&project, &library_path, &request, output.as_deref()),
                None => apply_library_device(&library, &request, output.as_deref()),
            }
        }
    }
}

fn apply_library_device(library_path: &std::path::Path, request: &flashmob::library::ApplyDevice, output: Option<&std::path::Path>) -> ExitCode {
    let device = match std::fs::read_to_string(library_path).map_err(|err| format!("could not read {}: {err}", library_path.display()))
        .and_then(|text| flashmob::library::Library::from_json(&text))
        .and_then(|library| flashmob::library::apply_device(&library, request)) {
        Ok(device) => device,
        Err(err) => return fail(&err),
    };
    match serde_json::to_value(&device) {
        Ok(value) => write_json(output, &value, false),
        Err(err) => fail(&err.to_string()),
    }
}

fn apply_library_project(project_path: &std::path::Path, library_path: &str, request: &flashmob::library::ApplyDevice, output: Option<&std::path::Path>) -> ExitCode {
    let project = match read_project(&project_path.to_path_buf()) {
        Ok(project) => project,
        Err(err) => return fail(&err),
    };
    let response = flashmob::exec::apply(project, &[flashmob::exec::Command::ApplyLibraryDevice {
        library_path: library_path.to_string(),
        device_id: request.device_id,
        id: request.id.clone(),
        bus: request.bus.clone(),
        name: request.name.clone(),
        rating_a: request.rating_a,
        current_base_a: request.current_base_a,
        voltage_v: request.voltage_v,
        function_name: request.function_name.clone(),
        protected_branch: request.protected_branch.clone(),
        library_name: request.library_name.clone(),
        fuse_total_clearing: request.fuse_total_clearing,
    }]);
    if !response.ok {
        return fail(response.error.as_deref().unwrap_or("could not apply the library device"));
    }
    match serde_json::to_value(&response.project) {
        Ok(value) => write_json(output, &value, false),
        Err(err) => fail(&err.to_string()),
    }
}

fn import_library(input: &std::path::Path, output: &std::path::Path, compact: bool, force: bool, redact: &[String]) -> ExitCode {
    if output.exists() && !force {
        return fail(&format!("{} exists; use --force to replace it", output.display()));
    }
    if std::fs::canonicalize(input).ok().is_some_and(|i| std::fs::canonicalize(output).ok() == Some(i)) {
        return fail("the output would replace the input file");
    }
    let bytes = match std::fs::read(input) {
        Ok(bytes) => bytes,
        Err(err) => return fail(&format!("could not read {}: {err}", input.display())),
    };
    let mut library = match flashmob::library_import::convert(&bytes) {
        Ok(library) => library,
        Err(err) => return fail(&err),
    };
    flashmob::library_import::redact(&mut library, redact);
    let text = if compact { serde_json::to_string(&library) } else { serde_json::to_string_pretty(&library) };
    let text = match text {
        Ok(text) => text,
        Err(err) => return fail(&err.to_string()),
    };
    if let Err(err) = std::fs::write(output, format!("{text}\n")) {
        return fail(&format!("could not write {}: {err}", output.display()));
    }
    emit(&serde_json::json!({"ok": true, "output": output.display().to_string(), "summary": library.summary}), false)
}

fn gui(project: Option<PathBuf>) -> ExitCode {
    #[cfg(feature = "gui")]
    {
        match ui::launch(project) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => fail(&err.to_string()),
        }
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = project;
        fail("this build has no window. Rebuild with the gui feature.")
    }
}

fn prepare(path: &PathBuf) -> Result<(Project, flashmob::study::StudyOutput), String> {
    let project = read_project(path)?;
    let results = study::run(&project, Studies::all())?;
    Ok((project, results))
}

fn read_project(path: &PathBuf) -> Result<Project, String> {
    let text = if path.as_os_str() == "-" {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text).map_err(|err| err.to_string())?;
        text
    } else {
        std::fs::read_to_string(path).map_err(|err| format!("could not read {}: {err}", path.display()))?
    };
    exec::load_project(&text)
}

fn emit(value: &serde_json::Value, compact: bool) -> ExitCode {
    write_json(None, value, compact)
}

fn emit_response(response: &ExecResponse, compact: bool, code: ExitCode) -> ExitCode {
    match serde_json::to_value(response) {
        Ok(value) => write_json(None, &value, compact).max_code(code),
        Err(err) => fail(&err.to_string()),
    }
}

trait ExitCombine {
    fn max_code(self, other: ExitCode) -> ExitCode;
}

impl ExitCombine for ExitCode {
    fn max_code(self, other: ExitCode) -> ExitCode {
        if exit_ok(other) { self } else { other }
    }
}

fn exit_ok(code: ExitCode) -> bool {
    code == ExitCode::SUCCESS
}

fn write_json(path: Option<&std::path::Path>, value: &serde_json::Value, compact: bool) -> ExitCode {
    let text = if compact {
        match serde_json::to_string(value) {
            Ok(v) => v,
            Err(err) => return fail(&err.to_string()),
        }
    } else {
        match serde_json::to_string_pretty(value) {
            Ok(v) => v,
            Err(err) => return fail(&err.to_string()),
        }
    };
    write_text_opt(path, &format!("{text}\n"))
}

fn write_text(path: &std::path::Path, text: &str) -> ExitCode {
    if path.as_os_str() == "-" {
        print!("{text}");
        let _ = std::io::stdout().flush();
        ExitCode::SUCCESS
    } else if let Err(err) = std::fs::write(path, text) {
        fail(&format!("could not write {}: {err}", path.display()))
    } else {
        ExitCode::SUCCESS
    }
}

fn write_text_opt(path: Option<&std::path::Path>, text: &str) -> ExitCode {
    match path {
        Some(path) => write_text(path, text),
        None => {
            print!("{text}");
            let _ = std::io::stdout().flush();
            ExitCode::SUCCESS
        }
    }
}

fn fail(message: &str) -> ExitCode {
    let body = serde_json::json!({"ok": false, "error": message});
    if let Ok(text) = serde_json::to_string_pretty(&body) {
        println!("{text}");
    }
    ExitCode::from(1)
}
