# SHA-256 — torajs example

Single-file SHA-256 implementation written in TypeScript. Tests
torajs against:

- 32-bit bit operations (`&`, `|`, `^`, `<<`, `>>`, `>>> 0`)
- Fixed-size `number[]` arrays
- Generic helpers (`number → number`)
- String-to-bytes conversion via `charCodeAt`
- Three known-answer tests (empty / "abc" / NIST 56-byte sample)

## Running

```sh
# AOT compile + run via cache
tr run sha256.ts

# Compile to a native binary
tr build sha256.ts -o sha256
./sha256
```

Expected output:

```
OK empty
OK abc
OK longer
```

## What this exercises

- `>>> 0` (unsigned right shift, zero-fill) as the canonical
  JS uint32 coercion idiom — needed for SHA-256 word arithmetic to
  match the reference vectors
- Multi-stage data flow: `string → number[] (bytes) → padded[] →
  message-schedule words → digest words → hex string`
- Long arithmetic-heavy loops (64-round compression × N blocks)
  — algorithm that benefits from AOT compilation

## Verification

The output digest is bit-identical to `bun run sha256.ts`. Both
match the published NIST FIPS 180-4 SHA-256 examples for the
empty input, `"abc"`, and the 56-byte
`abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq`.
