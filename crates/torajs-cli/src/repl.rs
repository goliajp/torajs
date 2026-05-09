//! V3-13 — `tr repl` interactive evaluator.
//!
//! Cross-line state preservation via source accumulation: every
//! input line is appended to a session buffer; the whole buffer
//! is recompiled + executed each turn, and only the new stdout
//! tail is shown to the user. Top-level `let` bindings, `function`
//! decls, `class` decls and `type` aliases all persist naturally
//! because they live in the buffer.
//!
//! This is the lowest-blast-radius shape that gives a usable REPL
//! on top of tora's whole-program compile pipeline. Incremental
//! compile (true REPL) is a follow-up — the compile time on a
//! growing buffer is dominated by LLVM, not by us, so the cost
//! is acceptable for interactive sessions of dozens of lines.
//!
//! Multi-line input: when the parse fails with an error that
//! suggests an unfinished construct (unclosed brace, expected
//! more tokens), the prompt switches to `... ` and reads more
//! lines until the buffer parses cleanly or the user enters a
//! blank line to abort.
//!
//! History persisted to `~/.torajs/repl_history`.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use torajs_core::{ast, check, lexer, parser};

const PROMPT_PRIMARY: &str = "tr> ";
const PROMPT_CONTINUE: &str = "...> ";

pub fn run() -> ExitCode {
    let history_path = history_path();
    let mut rl = match DefaultEditor::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("repl: cannot init readline: {e}");
            return ExitCode::from(1);
        }
    };
    if let Some(p) = &history_path {
        let _ = rl.load_history(p);
    }

    println!("torajs repl  ·  type `:help` for commands  ·  Ctrl-D to exit");
    let mut session = Session::new();
    let mut pending = String::new();

    loop {
        let prompt = if pending.is_empty() { PROMPT_PRIMARY } else { PROMPT_CONTINUE };
        let line = match rl.readline(prompt) {
            Ok(s) => s,
            Err(ReadlineError::Interrupted) => {
                if !pending.is_empty() {
                    pending.clear();
                    println!("(input cleared)");
                    continue;
                }
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!();
                break;
            }
            Err(e) => {
                eprintln!("repl: {e}");
                break;
            }
        };

        if pending.is_empty() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(cmd) = trimmed.strip_prefix(':') {
                handle_command(cmd, &mut session);
                continue;
            }
        }

        let _ = rl.add_history_entry(&line);
        if !pending.is_empty() {
            pending.push('\n');
        }
        pending.push_str(&line);

        // Wrap the new input as an expression-statement so a bare
        // expression like `1 + 2` echoes a value. Statements
        // (`let x = 1`, `function f() {}`) parse as themselves.
        let chunk = wrap_chunk(&pending);
        let candidate_src = session.candidate_source(&chunk);

        match parse_check(&candidate_src) {
            ParseOutcome::Ok => {
                pending.clear();
                session.commit(chunk.clone());
                if let Err(e) = session.run_and_print() {
                    eprintln!("error: {e}");
                }
            }
            ParseOutcome::Incomplete => {
                // Stay in continuation mode — caller types more.
            }
            ParseOutcome::Error(msg) => {
                eprintln!("error: {msg}");
                pending.clear();
            }
        }
    }

    if let Some(p) = &history_path {
        let _ = rl.save_history(p);
    }
    ExitCode::SUCCESS
}

fn handle_command(cmd: &str, session: &mut Session) {
    match cmd.trim() {
        "help" | "h" | "?" => {
            println!(":help / :h / :?    show this help");
            println!(":source / :s       print the accumulated session source");
            println!(":reset / :r        forget all bindings, start a fresh session");
            println!(":quit / :q         exit");
            println!();
            println!("Bare expressions print their value automatically:");
            println!("  tr> 1 + 2");
            println!("  3");
            println!();
            println!("Multi-line input continues with `...> ` until the buffer parses.");
        }
        "source" | "s" => {
            print!("{}", session.committed_source());
        }
        "reset" | "r" => {
            session.clear();
            println!("(session cleared)");
        }
        "quit" | "q" => {
            std::process::exit(0);
        }
        other => {
            eprintln!("unknown command `:{other}` — try `:help`");
        }
    }
}

struct Session {
    /// Accumulated user input, line-by-line. `run_and_print`
    /// joins these to compile.
    chunks: Vec<String>,
    /// Number of stdout lines emitted by the previous successful
    /// run. The next run's tail past this count is what the user
    /// sees as "this turn's output".
    last_output_lines: usize,
}

impl Session {
    fn new() -> Self {
        Self { chunks: Vec::new(), last_output_lines: 0 }
    }

    fn clear(&mut self) {
        self.chunks.clear();
        self.last_output_lines = 0;
    }

    fn committed_source(&self) -> String {
        // Synthesize a no-op top-level statement so a session
        // composed entirely of fn / type / class declarations
        // still gives synthesize_main something to put in `main`
        // (otherwise the linker errors on a missing `_main`
        // symbol). The sentinel is unobservable by the user —
        // it produces no output and leaves no usable binding.
        let mut s = String::from("let __torajs_repl_anchor: number = 0\n");
        for c in &self.chunks {
            s.push_str(c);
            if !c.ends_with('\n') {
                s.push('\n');
            }
        }
        s
    }

    fn candidate_source(&self, new_chunk: &str) -> String {
        let mut s = self.committed_source();
        s.push_str(new_chunk);
        if !new_chunk.ends_with('\n') {
            s.push('\n');
        }
        s
    }

    fn commit(&mut self, chunk: String) {
        self.chunks.push(chunk);
    }

    fn run_and_print(&mut self) -> Result<(), String> {
        let src = self.committed_source();
        let exe = std::env::current_exe()
            .map_err(|e| format!("locating tr binary: {e}"))?;
        let tmp = std::env::temp_dir().join(format!(
            "torajs-repl-{}-{}.ts",
            std::process::id(),
            rand_suffix()
        ));
        fs::write(&tmp, &src)
            .map_err(|e| format!("writing temp source: {e}"))?;
        let out = Command::new(&exe)
            .arg("run")
            .arg(&tmp)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("spawning tr run: {e}"))?;
        let _ = fs::remove_file(&tmp);

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // Roll back the chunk that just broke compilation: a
            // failed run shouldn't poison the session forever.
            self.chunks.pop();
            return Err(stderr.trim_end().to_string());
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let lines: Vec<&str> = stdout.split_inclusive('\n').collect();
        let new_lines = if lines.len() > self.last_output_lines {
            &lines[self.last_output_lines..]
        } else {
            &[][..]
        };
        for ln in new_lines {
            print!("{ln}");
        }
        self.last_output_lines = lines.len();
        Ok(())
    }
}

enum ParseOutcome {
    Ok,
    Incomplete,
    Error(String),
}

fn parse_check(src: &str) -> ParseOutcome {
    let tokens = match lexer::tokenize(src) {
        Ok(t) => t,
        Err(e) => return ParseOutcome::Error(format!("lex: {e}")),
    };
    let mut a = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => {
            if looks_like_unfinished(&e) {
                return ParseOutcome::Incomplete;
            }
            return ParseOutcome::Error(format!("parse: {e}"));
        }
    };
    a.source = src.to_string();
    a.warm_newline_cache();
    ast::desugar_classes(&mut a);
    ast::lift_arrow_fns(&mut a);
    ast::synthesize_forwarders(&mut a);
    ast::desugar_uninit_let(&mut a);
    if let Err(e) = check::check(&a) {
        return ParseOutcome::Error(format!("type: {e}"));
    }
    ParseOutcome::Ok
}

fn looks_like_unfinished(err: &str) -> bool {
    let needle = err.to_ascii_lowercase();
    needle.contains("unexpected eof")
        || needle.contains("expected `}`")
        || needle.contains("expected `)`")
        || needle.contains("expected `]`")
        || needle.contains("expected `;`")
        || needle.contains("expected expression")
        || needle.contains("got eof")
}

/// Tora doesn't have an expression-statement form that prints by
/// default, so for bare-expression input (no `let`, `function`,
/// `class`, `type`, `if`, etc) we wrap with `console.log(...)` so
/// the REPL echoes the value. Heuristic: leading keyword check on
/// the trimmed input. Misclassifications (e.g. `x = 1` falls
/// through the keyword check) get the wrap and produce a
/// `console.log(x = 1)` — still valid: assignment expression
/// returns the assigned value.
fn wrap_chunk(input: &str) -> String {
    let trimmed = input.trim();
    if is_statement_shape(trimmed) {
        return input.to_string();
    }
    // Strip a single trailing semicolon so `console.log(2+3;)` doesn't form.
    let body = trimmed.trim_end_matches(';').trim_end();
    format!("console.log({body})\n")
}

fn is_statement_shape(input: &str) -> bool {
    const KEYS: &[&str] = &[
        "let ", "const ", "var ", "function ", "class ", "type ", "import ",
        "export ", "if ", "for ", "while ", "do ", "switch ", "return ",
        "throw ", "try ", "{", "//", "async ", "interface ",
    ];
    KEYS.iter().any(|k| input.starts_with(k))
        || input.contains('\n') // multi-line input is almost always a block
}

fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".torajs");
    let _ = fs::create_dir_all(&dir);
    Some(dir.join("repl_history"))
}

fn rand_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{n:x}")
}
