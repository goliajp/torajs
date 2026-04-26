mod ast;
mod check;
mod interp;
mod ir;
mod lexer;
mod lower;
mod parser;
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
        Some("run") => run_pipeline(args.get(1), Stage::Run),
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
    println!("    run <file>          lex, parse, check, lower, and execute");
    println!("    tokenize <file>     print the token stream");
    println!("    parse <file>        print the parsed AST");
    println!("    check <file>        type-check, exit nonzero on error");
    println!("    ir <file>           print the lowered IR");
    println!();
    println!("    --version, -V       print version");
    println!("    --help, -h          print this help");
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
