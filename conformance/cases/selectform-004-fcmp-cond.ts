// RFC 20260719-select-formation -- FCmp-cond fusion. A select whose
// condition is a float compare used to pay cset + cmp #0 between the
// FCMP and the CSEL/FCSEL; the fuse re-emits the compare at the
// select so the predicate rides NZCV. The F64-value case is gated on
// both compare operands sitting in allocated FPRs (the FP scratches
// hold the value arms). Semantics must be identical with the sink on,
// off (TORAJS_CMP_SINK_OFF=1), and under tr run.
function pickMax(n: number): number {
  let best = 0;
  let x = 1.5;
  for (let i = 0; i < n; i++) {
    x = x * 1.1 - Math.floor(x);
    best = x > best ? x + 0.5 : best - 0.25;
  }
  return best;
}
// Unordered != (JS !=) maps to a single cond (NE) and fuses; NaN
// makes the condition true and must keep taking the then arm.
function neqNaN(n: number): number {
  let c = 0;
  let x = 0.5;
  for (let i = 0; i < n; i++) {
    x = i === 500 ? NaN : x * 3.9 * (1 - x);
    c = x != 0.5 ? c + 2 : c - 1;
    if (i === 500) {
      x = 0.25;
    }
  }
  return c;
}
// NaN through an ordered compare: the condition is false, the else
// arm must win -- fusion may not flip the unordered behavior.
function nanOrdered(): number {
  const x: number = NaN;
  const a = 7;
  const b = 3;
  return x > 0.5 ? a : b;
}
console.log(pickMax(1000), pickMax(3));
console.log(neqNaN(1000), neqNaN(1));
console.log(nanOrdered());
