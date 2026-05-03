# Getting started with torajs

## Install

```sh
# easiest path — fetches the latest release for your platform
curl -fsSL https://install.torajs.com | bash

export PATH="$HOME/.torajs/bin:$PATH"
tr --version
```

Or pin a specific tarball directly:

```sh
curl -L https://github.com/goliajp/torajs/releases/download/v0.1.0-beta/tr-v0.1.0-beta-darwin-arm64.tar.gz \
  | tar -xz -C ~/.torajs/bin/
```

(Source builds: `cargo build --release -p tr` from the repo root
produces `target/release/tr`.)

## Hello world

```sh
cat > hello.ts <<'EOF'
console.log("hello, torajs");
EOF

# JIT-style: compile + run, cached at ~/.torajs/cache
tr run hello.ts
# → hello, torajs

# AOT: compile to a native binary
tr build hello.ts -o hello
./hello
# → hello, torajs
```

## A more realistic example

```ts
// fizz-buzz.ts
function fizzBuzz(n: number): void {
  for (let i = 1; i <= n; i++) {
    if (i % 15 === 0) console.log("FizzBuzz");
    else if (i % 3 === 0) console.log("Fizz");
    else if (i % 5 === 0) console.log("Buzz");
    else console.log(i.toString());
  }
}

fizzBuzz(20);
```

```sh
tr run fizz-buzz.ts
```

More examples in [`examples/`](../examples/) — each is a complete
self-contained `.ts` file with run instructions.

## Commands

```sh
tr run <file>             # JIT-style: compile (with cache) and execute
tr build <file> -o <out>  # AOT: produce a native binary
                          #   --opt O0|O1|O2|O3 (default O3)
tr check <file>           # typecheck only, exit non-zero on error
tr parse <file>           # print the AST
tr ssa <file>             # print the lowered SSA IR
tr tokenize <file>        # print the token stream
tr --version              # print version
tr --help                 # show help
```

## What works today

See [`language-status.md`](./language-status.md) for the full
feature table. In short: most everyday TS works — classes, generics,
closures, JSON, multi-file imports, full string/array/Math stdlib,
try/catch. The reference baseline is `bun`; if bun runs your code
and tr rejects it, that's a roadmap gap to fix.

## Verifying parity with `bun`

```sh
diff <(bun run yourfile.ts) <(tr run yourfile.ts)
```

Empty diff means tr's output matches bun byte-for-byte.

## Where to file feedback

- Issues / feature requests: `github.com/goliajp/torajs/issues`
- Performance regressions: include the bench case + `tr --version`
  + your CPU model in the report
- Anything bun runs that tr rejects: include the `.ts` source +
  the first stderr line tr produces

## Cache

`tr run` caches compiled binaries at `~/.torajs/cache/<hash>`. The
hash incorporates the source file, all imported files (transitive),
and the compiler version. To force a fresh compile, set
`TORAJS_NO_CACHE=1`:

```sh
TORAJS_NO_CACHE=1 tr run x.ts
```

## Performance

Bench scoreboard + reproduction steps: [`perf.md`](./perf.md).
