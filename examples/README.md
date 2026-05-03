# torajs examples

Self-contained TypeScript programs that run end-to-end on `tr`.
Each example has its own README with run instructions and a list of
torajs features it exercises. Output is verified against
`bun run <example>.ts` — anything tr accepts produces bun-identical
output.

## Index

| Example | What it shows |
|---|---|
| [`sha256/`](sha256/) | NIST FIPS 180-4 SHA-256 with bit ops + `>>>` UInt32 coercion |
| [`prime-sieve/`](prime-sieve/) | Sieve of Eratosthenes on `boolean[]` |
| [`fizz-buzz/`](fizz-buzz/) | Classic FizzBuzz, `Number → string` + modulo |
| [`wc-clone/`](wc-clone/) | POSIX `wc`-style line/word/byte counts via `charCodeAt` |

## Running

```sh
# JIT-style: compile and execute (cached at ~/.torajs/cache)
tr run <example>.ts

# AOT: compile to a native binary, then exec the binary
tr build <example>.ts -o <out>
./<out>
```

Both paths use the same compiler; `tr run` writes the cached binary
to `~/.torajs/cache/<hash>` and execs it, while `tr build` lets you
pick the output path explicitly.

## Verifying parity with `bun`

```sh
diff <(bun run <example>.ts) <(tr run <example>.ts)
```

Empty diff means tr's output matches bun byte-for-byte.
