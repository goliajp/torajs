// P3.3-a — exercise the new pure-Rust drop / drop_rc path in
// torajs-bigint (formerly runtime_bigint.c's free + rc_dec wrap).
//
// BigInt literals allocate via C-side `bigint_from_decimal` (still
// in runtime_bigint.c, scheduled for P3.3-b). Bindings going out of
// scope route through ssa_lower's emit_drop_value Type::BigInt
// → `__torajs_bigint_drop_rc` (now Rust). Subsequent rc_dec hitting
// zero invokes `__torajs_bigint_drop` (now Rust → libc free). This
// fixture's bun-parity ensures the cross-tier hand-off is bit-identical
// to the pre-port behavior.

// Basic literal alloc + drop path.
{
  const a: bigint = 1n
  const b: bigint = 2n
  console.log(typeof a, typeof b) // bigint bigint
}

// Arith creates a fresh BigInt that drops at end of block.
{
  const sum: bigint = 100n + 200n + 300n
  console.log(typeof sum) // bigint
}

// Negative literal.
{
  const neg: bigint = -5n
  console.log(typeof neg) // bigint
}

// Many short-lived BigInts in a row exercise the alloc/drop tightloop.
function makeTen(): bigint {
  return 1n + 2n + 3n + 4n
}
for (let i = 0; i < 10; i++) {
  const x: bigint = makeTen()
  if (i === 9) console.log(typeof x) // bigint (printed once)
}

// Pass-by-binding — rc_inc on assign, rc_dec on each scope exit.
{
  const a: bigint = 42n
  const b: bigint = a
  const c: bigint = b
  console.log(typeof a, typeof b, typeof c) // bigint bigint bigint
}
