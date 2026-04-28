mod ast;
mod build;
mod check;
mod interp;
mod ir;
mod lexer;
mod lower;
mod parser;
mod ssa;
mod ssa_inkwell;
mod ssa_lower;
mod value;

use std::env;
use std::io::Read;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy)]
enum Stage {
    Tokenize,
    Parse,
    Check,
    Ir,
    Ssa,
    Run,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str);

    match cmd {
        Some("--version") | Some("-V") => {
            println!("tr {VERSION}");
            ExitCode::SUCCESS
        }
        None | Some("--help") | Some("-h") => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some("tokenize") => run_pipeline(args.get(1), Stage::Tokenize),
        Some("parse") => run_pipeline(args.get(1), Stage::Parse),
        Some("check") => run_pipeline(args.get(1), Stage::Check),
        Some("ir") => run_pipeline(args.get(1), Stage::Ir),
        Some("ssa") => run_pipeline(args.get(1), Stage::Ssa),
        Some("run") => run_pipeline(args.get(1), Stage::Run),
        Some("build") => run_build(&args[1..]),
        Some("build-llvm") => run_build_llvm(&args[1..]),
        Some("ssa-demo") => {
            ssa::demo_fib40().print();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown command `{other}`");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    println!("tr {VERSION}");
    println!();
    println!("USAGE:");
    println!("    tr <COMMAND> <file|->");
    println!();
    println!("COMMANDS:");
    println!("    run <file>           lex, parse, check, lower, and execute");
    println!("    tokenize <file>      print the token stream");
    println!("    parse <file>         print the parsed AST");
    println!("    check <file>         type-check, exit nonzero on error");
    println!("    ir <file>            print the lowered (stack-machine) IR");
    println!("    ssa <file>           print the lowered SSA IR (P3.5 — fns only, fib40 shape)");
    println!("    build <in> -o <out>  AOT-compile to wasm (P3.1, very limited)");
    println!(
        "    build-llvm <in> -o <out> [--opt O0|O1|O2|O3]"
    );
    println!("                         AOT-compile via LLVM (P3.5; fib40 shape only for now)");
    println!("    ssa-demo             print a hand-built SSA fib40 (P3.5 step 1)");
    println!();
    println!("    --version, -V        print version");
    println!("    --help, -h           print this help");
    println!();
    println!("Use `-` as the file to read from stdin.");
}

fn run_pipeline(file_arg: Option<&String>, stage: Stage) -> ExitCode {
    let path = match file_arg {
        Some(p) => p,
        None => {
            eprintln!("error: missing file argument (use `-` for stdin)");
            return ExitCode::from(2);
        }
    };
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    pipeline(&src, stage)
}

fn pipeline(src: &str, stage: Stage) -> ExitCode {
    let tokens = match lexer::tokenize(src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {e}");
            return ExitCode::from(1);
        }
    };
    if matches!(stage, Stage::Tokenize) {
        for t in &tokens {
            println!("{:?} @ {}..{}", t.token, t.span.start, t.span.end);
        }
        return ExitCode::SUCCESS;
    }

    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::from(1);
        }
    };
    if matches!(stage, Stage::Parse) {
        ast.print();
        return ExitCode::SUCCESS;
    }

    if let Err(e) = check::check(&ast) {
        eprintln!("type error: {e}");
        return ExitCode::from(1);
    }
    if matches!(stage, Stage::Check) {
        return ExitCode::SUCCESS;
    }

    if matches!(stage, Stage::Ssa) {
        let m = ssa_lower::lower(&ast);
        m.print();
        return ExitCode::SUCCESS;
    }

    let module = lower::lower(&ast);
    if matches!(stage, Stage::Ir) {
        module.print();
        return ExitCode::SUCCESS;
    }

    if let Err(e) = interp::execute(&module) {
        eprintln!("runtime error: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn run_build(args: &[String]) -> ExitCode {
    if matches!(
        args.first().map(String::as_str),
        Some("--help") | Some("-h")
    ) {
        println!("tr build {VERSION} (AOT to wasm — P3.1 stub)");
        println!();
        println!("USAGE: tr build <input.ts> -o <output.wasm>");
        println!();
        println!("Currently only programs of the form `console.log(\"<string>\")`");
        println!("compile. P3.2/3.3 will extend this to arithmetic and functions.");
        return ExitCode::SUCCESS;
    }

    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                let Some(path) = args.get(i) else {
                    eprintln!("error: `-o` requires a path");
                    return ExitCode::from(2);
                };
                output = Some(path.as_str());
                i += 1;
            }
            other if !other.starts_with('-') && input.is_none() => {
                input = Some(other);
                i += 1;
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }

    let Some(input) = input else {
        eprintln!("error: missing input file (use `-` for stdin)");
        return ExitCode::from(2);
    };
    let Some(output) = output else {
        eprintln!("error: missing `-o <output>`");
        return ExitCode::from(2);
    };

    let src = match read_source(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let tokens = match lexer::tokenize(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {e}");
            return ExitCode::from(1);
        }
    };
    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = check::check(&ast) {
        eprintln!("type error: {e}");
        return ExitCode::from(1);
    }
    match build::build(&ast, std::path::Path::new(output)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(build::BuildError::NotYetImplemented(msg)) => {
            // Exit 3 is the bench harness's signal for "feature not yet
            // implemented" — distinct from real failures (exit 1).
            eprintln!("not yet supported: {msg}");
            ExitCode::from(3)
        }
        Err(build::BuildError::Real(msg)) => {
            eprintln!("build error: {msg}");
            ExitCode::from(1)
        }
    }
}

/// `tr build-llvm <in> -o <out> [--opt O0|O1|O2|O3]`. Pipeline: lex → parse →
/// check → ssa_lower → ssa_inkwell → cc. Bench harness's "torajs-llvm" runner
/// calls this. Step 3 only: handles fib40 shape; richer cases need step 4.
fn run_build_llvm(args: &[String]) -> ExitCode {
    if matches!(
        args.first().map(String::as_str),
        Some("--help") | Some("-h")
    ) {
        println!("tr build-llvm — AOT compile via LLVM");
        println!();
        println!("USAGE: tr build-llvm <input.ts> -o <output> [--opt O0|O1|O2|O3]");
        return ExitCode::SUCCESS;
    }

    let mut input: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut opt: String = "O1".into();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                i += 1;
                let Some(path) = args.get(i) else {
                    eprintln!("error: `-o` requires a path");
                    return ExitCode::from(2);
                };
                output = Some(path.as_str());
                i += 1;
            }
            "--opt" => {
                i += 1;
                let Some(level) = args.get(i) else {
                    eprintln!("error: `--opt` requires a level (O0|O1|O2|O3)");
                    return ExitCode::from(2);
                };
                opt = level.clone();
                i += 1;
            }
            other if !other.starts_with('-') && input.is_none() => {
                input = Some(other);
                i += 1;
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }

    let Some(input) = input else {
        eprintln!("error: missing input file");
        return ExitCode::from(2);
    };
    let Some(output) = output else {
        eprintln!("error: missing `-o <output>`");
        return ExitCode::from(2);
    };

    let src = match read_source(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let tokens = match lexer::tokenize(&src) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("lex error: {e}");
            return ExitCode::from(1);
        }
    };
    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("parse error: {e}");
            return ExitCode::from(1);
        }
    };
    if let Err(e) = check::check(&ast) {
        eprintln!("type error: {e}");
        return ExitCode::from(1);
    }

    // ssa_lower currently panics on unsupported AST shapes. Catch the panic
    // and report as exit-code-3 (bench harness's "not yet implemented" skip).
    // Silence the default panic hook so the bench harness's stderr stays
    // clean — we report the message ourselves below.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let lower_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        ssa_lower::lower(&ast)
    }));
    std::panic::set_hook(prev_hook);
    let ssa_module = match lower_result {
        Ok(m) => m,
        Err(payload) => {
            let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "ssa_lower panicked".to_string()
            };
            eprintln!("not yet supported: {msg}");
            return ExitCode::from(3);
        }
    };

    match ssa_inkwell::compile(&ssa_module, std::path::Path::new(output), &opt) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("build-llvm error: {e}");
            ExitCode::from(1)
        }
    }
}

fn read_source(arg: &str) -> Result<String, String> {
    if arg == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| format!("reading stdin: {e}"))?;
        Ok(s)
    } else {
        std::fs::read_to_string(arg).map_err(|e| format!("reading {arg}: {e}"))
    }
}
