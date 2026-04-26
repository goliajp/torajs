# startup

Pure runtime startup overhead. Each implementation prints `x\n` and exits.

There is no algorithmic work — what we measure is the cost of:

- spawning the runtime process
- loading whatever stdlib it needs to do `console.log` / `print` / `println!` / `fmt.Println`
- compiling/parsing the source (for interpreted runtimes; included in `run_ms`)
- exiting

For compiled languages (rust, go) the compile step is timed separately as `compile_ms` and the resulting binary is what's measured for `run_ms`.

## Per-language notes

- `main.ts` — used by both `bun` and `node`. Plain `console.log("x")`. Node 22.6+ strips TS types on the fly, so the file works for both.
- `main.py` — `print("x")`.
- `main.rs` — `fn main() { println!("x") }`. Compiled with `rustc -C opt-level=3`.
- `main.go` — uses `fmt.Println`. Compiled with `go build`.
