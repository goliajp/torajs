# Prime sieve — torajs example

Sieve of Eratosthenes up to `N = 1000`. Prints the count of primes
≤ N and the first / last 10 primes for verification.

## Running

```sh
tr run prime-sieve.ts
# or compile to a native binary
tr build prime-sieve.ts -o sieve && ./sieve
```

Expected output:

```
primes <= 1000: 168
first 10: 2, 3, 5, 7, 11, 13, 17, 19, 23, 29
last 10:  937, 941, 947, 953, 967, 971, 977, 983, 991, 997
```

## Exercises

- Large `boolean[]` array (`isComposite`) with index assignment
- Nested `for` / `while` loops
- Integer arithmetic + comparison (`i * i <= n`)
- Dynamic array `push`
- Multi-step pipeline: build sieve → collect primes → format output

Output matches `bun run prime-sieve.ts` exactly.
