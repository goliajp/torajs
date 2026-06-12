// chunk 2.5 F3 (ann-width rfc §5.6) — generator step value slot
// width negotiation. The synthesized `__step_<gen>` alias joins
// field widths nominally (single producer: the state machine's
// yield returns), so a generator whose yields are f64-possible gets
// an f64 value slot end-to-end: the yield ObjectLit, the `.value`
// reads, and the `yield*` delegation drain all agree. Pre-F3 the
// alias parsed `value: number` to i64 while the yield wrote f64 —
// SIGABRT through the delegation path.
function* halves(n: number): number {
  for (let k = 1; k <= n; k++) {
    yield k / 2;
  }
}
function* outer(n: number): number {
  yield* halves(n);
}
let s = 0;
for (const v of outer(3)) {
  s = s + v;
}
console.log(s);

let g = halves(2);
console.log(g.next().value);
console.log(g.next().value);
console.log(g.next().done);

// all-int generator holds the narrow slot
function* counts(n: number): number {
  for (let k = 0; k < n; k++) {
    yield k * 2;
  }
}
let t = 0;
for (const c of counts(4)) {
  t = t + c;
}
console.log(t);
