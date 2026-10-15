use crate::VERSION;

pub(crate) fn print_usage() {
    println!("tr {VERSION}");
    println!();
    println!("USAGE:");
    println!("    tr <COMMAND> <file|->");
    println!();
    println!("COMMANDS:");
    println!("    run <file>           AOT-compile and execute");
    println!("    jit <file>           alias for `run` (back-compat)");
    println!("    tokenize <file>      print the token stream");
    println!("    parse <file>         print the parsed AST");
    println!("    check <file>         type-check, exit nonzero on error");
    println!("    ssa <file>           print the lowered SSA IR");
    println!("    build <in> -o <out>");
    println!("                         AOT-compile via in-house aarch64 toolchain → native binary");
    println!("    ssa-demo             print a hand-built SSA fib40 (P3.5 step 1 leftover)");
    println!("    lsp                  speak Language Server Protocol over stdio");
    println!("    lsp-bench            measure LSP latency on a synthetic 1K-line fixture");
    println!(
        "    repl                 launch interactive evaluator (history at ~/.torajs/repl_history)"
    );
    println!(
        "    debug <file>         compile with DWARF + drop into lldb (set breakpoints, step, inspect)"
    );
    println!(
        "    fmt <file> [--write] reformat source to tr's canonical style (stdout, or in-place with --write)"
    );
    println!(
        "    lint <file> [--deny] surface 5 lint warnings (unused-let, dead-code-after-return, unreachable-catch, shadowed-let, unused-import); --deny exits non-zero on any warning"
    );
    println!("    cache size           print ~/.torajs/cache size");
    println!(
        "    cache clean [--max-mb N]  LRU-prune ~/.torajs/cache to under N MB (default 2048)"
    );
    println!();
    println!("    --version, -V        print version");
    println!("    --help, -h           print this help");
    println!();
    println!("Use `-` as the file to read from stdin.");
}
