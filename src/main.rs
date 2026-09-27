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
For a model or any other headless caller, stdout of schema, sample, validate, run, exec, sld, and tcc is data and there are no prompts. \
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
        /// Project JSON to open. Omit to start with the built-in sample.
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
    /// Write an engineering data package for mapping (PTW import unverified).
    ExportSkm {
        path: PathBuf,
        /// Folder for the export, or an .xml file path.
        #[arg(short, long)]
        output: PathBuf,
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
        Command::ExportSkm { path, output } => match read_project(&path) {
            Ok(project) => match flashmob::skm::write_dir(&project, output.as_path()) {
                Ok(dir) => {
                    println!("{}", serde_json::json!({"ok": true, "directory": dir.display().to_string()}));
                    ExitCode::SUCCESS
                }
                Err(err) => fail(&err),
            },
            Err(err) => fail(&err),
        },
    }
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
