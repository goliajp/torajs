# wc-clone — torajs example

Counts lines, words, and bytes for a set of hardcoded text samples,
mirroring POSIX `wc`'s output format (`L W B label`).

## Running

```sh
tr run wc-clone.ts
# or compile to a native binary
tr build wc-clone.ts -o wc && ./wc
```

Expected output:

```
   1   2   12 sample1
   2   9   44 sample2
   4  10   50 sample3
```

## Exercises

- String iteration via `charCodeAt`
- ASCII whitespace classification
- Multiple counters threaded through a single pass
- Right-aligned padded output (`pad` helper)
- Returning multi-value results as `number[]`

Output matches `bun run wc-clone.ts` exactly.
