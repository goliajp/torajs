use std::env;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

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
        Some(sub @ ("run" | "tokenize" | "parse" | "check" | "ir")) => {
            eprintln!("error: `{sub}` is not implemented yet (placeholder)");
            ExitCode::from(2)
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
    println!("    tr <COMMAND> [args]");
    println!();
    println!("COMMANDS:");
    println!("    run <file.ts>       lex, parse, check, lower, and execute");
    println!("    tokenize <file>     print the token stream");
    println!("    parse <file>        print the parsed AST");
    println!("    check <file>        type-check, exit nonzero on error");
    println!("    ir <file>           print the lowered IR");
    println!();
    println!("    --version, -V       print version");
    println!("    --help, -h          print this help");
}
