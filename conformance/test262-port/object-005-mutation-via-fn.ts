// Adapted from test262: object passed to a function that mutates a
// field — caller observes the mutation when reading the field after.
// (One call only — tr's affine ownership doesn't yet allow re-passing
// the same struct binding to a value-param function on consecutive
// calls; that lands when method dispatch is generalized in M-OO.3+.)
type Counter = { value: number };

function increment(c: Counter): number {
  c.value = c.value + 1;
  return c.value;
}

function check(): number {
  let c: Counter = { value: 10 };
  if (increment(c) !== 11) { throw "#1"; }
  if (c.value !== 11) { throw "#2: caller sees mutation"; }
  return 0;
}
console.log(check());
