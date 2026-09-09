// A KEYED store raises too, and the may-throw analysis has to know it
// or the caller prunes its check away. Neither shape here is exotic.
//
// `xs[0] = 99` over a FROZEN array raised in the callee — which
// correctly left the element alone — and then the caller printed the
// ret sentinel, so `console.log(w(a))` answered `null` where every
// engine throws.
function w(xs) {
  xs[0] = 99;
  return "after";
}
const a = [1, 2, 3];
Object.freeze(a);
try {
  console.log(w(a));
} catch (e) {
  console.log("threw", (e as Error).constructor.name);
}
console.log("a[0] =", a[0]);

// `n[0]++` over a primitive was a compile refusal — "not yet
// supported: post-incr index on non-array I64" — while the same
// program through an `any` binding threw. The keyed any lane already
// answers it, so the typed spelling now boxes into it.
function bump(n) {
  n[0]++;
  return "after";
}
try {
  console.log(bump(1));
} catch (e) {
  console.log("threw", (e as Error).constructor.name);
}
try {
  console.log(bump(true));
} catch (e) {
  console.log("threw", (e as Error).constructor.name);
}
const s: any = "ab";
try {
  s[0]++;
  console.log("no throw");
} catch (e) {
  console.log("threw", (e as Error).constructor.name);
}
