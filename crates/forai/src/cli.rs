use std::env;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum CliCommand {
    Compile {
        source: PathBuf,
        out: Option<PathBuf>,
        compact: bool,
    },
    Build {
        dir: PathBuf,
        debug: bool,
    },
    Run {
        source: Option<PathBuf>,
        input: Option<PathBuf>,
        report: Option<PathBuf>,
        args: Vec<String>,
        debug: bool,
        debug_port: u16,
    },
    RunWasm {
        wasm_path: PathBuf,
    },
    Test {
        path: PathBuf,
    },
    Doc {
        path: PathBuf,
        out: Option<PathBuf>,
    },
    Fmt {
        path: PathBuf,
        check: bool,
    },
    Lsp,
    Mcp,
    New { name: String },
    Help,
}

pub fn usage() -> &'static str {
    "Usage:
  forai new <name>                        create a new project
  forai build [dir] [--debug]             build project (requires forai.json)
  forai run [source.fa] [args...]         interpreted run
  forai run --debug [--port N]            run with interactive debugger (default port 4810)
  forai run --wasm <file.wasm>            run WASM artifact via wasmtime
  forai test <path>                       run test blocks
  forai doc <path> [-o <out.json>]        generate docs
  forai fmt [path] [--check]              format .fa files (or check formatting)
  forai compile <source.fa> [-o <out.json>] [--compact]
  forai lsp                               start language server (stdio)
  forai mcp                               start MCP server (stdio)"
}

fn parse_build_args(args: &[String]) -> Result<CliCommand, String> {
    let mut dir: Option<PathBuf> = None;
    let mut debug = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--debug" => {
                debug = true;
                i += 1;
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            flag if flag.starts_with('-') => return Err(format!("Unknown flag: {flag}")),
            raw => {
                if dir.is_some() {
                    return Err("Only one directory argument is supported".to_string());
                }
                dir = Some(PathBuf::from(raw));
                i += 1;
            }
        }
    }

    let dir = dir.unwrap_or_else(|| PathBuf::from("."));
    Ok(CliCommand::Build { dir, debug })
}

fn parse_compile_args(args: &[String]) -> Result<CliCommand, String> {
    let mut source: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut compact = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                if i + 1 >= args.len() {
                    return Err("Missing value for -o/--out".to_string());
                }
                out = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--compact" => {
                compact = true;
                i += 1;
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            flag if flag.starts_with('-') => return Err(format!("Unknown flag: {flag}")),
            raw => {
                if source.is_some() {
                    return Err("Only one source file is supported".to_string());
                }
                source = Some(PathBuf::from(raw));
                i += 1;
            }
        }
    }

    let Some(source) = source else {
        return Err(usage().to_string());
    };
    Ok(CliCommand::Compile {
        source,
        out,
        compact,
    })
}

fn parse_run_args(args: &[String]) -> Result<CliCommand, String> {
    let mut source: Option<PathBuf> = None;
    let mut input: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut wasm_mode = false;
    let mut debug = false;
    let mut debug_port: u16 = 4810;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--wasm" => {
                wasm_mode = true;
                i += 1;
            }
            "--debug" => {
                debug = true;
                i += 1;
            }
            "--port" => {
                if i + 1 >= args.len() {
                    return Err("Missing value for --port".to_string());
                }
                debug_port = args[i + 1]
                    .parse()
                    .map_err(|_| format!("Invalid port number: {}", args[i + 1]))?;
                i += 2;
            }
            "--input" => {
                if i + 1 >= args.len() {
                    return Err("Missing value for --input".to_string());
                }
                input = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--report" => {
                if i + 1 >= args.len() {
                    return Err("Missing value for --report".to_string());
                }
                report = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            flag if flag.starts_with('-') => return Err(format!("Unknown flag: {flag}")),
            raw => {
                if source.is_none() {
                    source = Some(PathBuf::from(raw));
                } else {
                    positional.push(raw.to_string());
                }
                i += 1;
            }
        }
    }

    if wasm_mode {
        let Some(source) = source else {
            return Err("--wasm requires a path to a .wasm file".to_string());
        };
        return Ok(CliCommand::RunWasm { wasm_path: source });
    }

    Ok(CliCommand::Run {
        source,
        input,
        report,
        args: positional,
        debug,
        debug_port,
    })
}

fn parse_test_args(args: &[String]) -> Result<CliCommand, String> {
    if args.is_empty() {
        return Ok(CliCommand::Test {
            path: PathBuf::from("."),
        });
    }
    if args.len() == 1 {
        return Ok(CliCommand::Test {
            path: PathBuf::from(&args[0]),
        });
    }
    Err("`test` accepts at most one path argument".to_string())
}

fn parse_doc_args(args: &[String]) -> Result<CliCommand, String> {
    let mut path: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                if i + 1 >= args.len() {
                    return Err("Missing value for -o/--out".to_string());
                }
                out = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            flag if flag.starts_with('-') => return Err(format!("Unknown flag: {flag}")),
            raw => {
                if path.is_some() {
                    return Err("Only one path argument is supported".to_string());
                }
                path = Some(PathBuf::from(raw));
                i += 1;
            }
        }
    }

    let Some(path) = path else {
        return Err(usage().to_string());
    };
    Ok(CliCommand::Doc { path, out })
}

fn parse_fmt_args(args: &[String]) -> Result<CliCommand, String> {
    let mut path: Option<PathBuf> = None;
    let mut check = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => {
                check = true;
                i += 1;
            }
            "-h" | "--help" => return Ok(CliCommand::Help),
            flag if flag.starts_with('-') => return Err(format!("Unknown flag: {flag}")),
            raw => {
                if path.is_some() {
                    return Err("Only one path argument is supported".to_string());
                }
                path = Some(PathBuf::from(raw));
                i += 1;
            }
        }
    }

    let path = path.unwrap_or_else(|| PathBuf::from("."));
    Ok(CliCommand::Fmt { path, check })
}

fn parse_new_args(args: &[String]) -> Result<CliCommand, String> {
    if args.len() != 1 || args[0].starts_with('-') {
        return Err("Usage: forai new <project-name>".to_string());
    }
    Ok(CliCommand::New { name: args[0].clone() })
}

pub fn parse_cli() -> Result<CliCommand, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        return Ok(CliCommand::Help);
    }

    match args[0].as_str() {
        "help" | "-h" | "--help" => Ok(CliCommand::Help),
        "build" => parse_build_args(&args[1..]),
        "compile" => parse_compile_args(&args[1..]),
        "run" => parse_run_args(&args[1..]),
        "test" => parse_test_args(&args[1..]),
        "doc" => parse_doc_args(&args[1..]),
        "fmt" => parse_fmt_args(&args[1..]),
        "lsp" => Ok(CliCommand::Lsp),
        "mcp" => Ok(CliCommand::Mcp),
        "new" => parse_new_args(&args[1..]),
        _ => Err(format!("Unknown command `{}`.\\n{}", args[0], usage())),
    }
}
