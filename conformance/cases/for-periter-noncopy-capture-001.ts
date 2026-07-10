// RFC 20260710 C3 (chunk 749) — a MUTATED non-Copy for-init binding
// rides the per-iteration box rewrite (ES §14.7.4.9): closures
// pushed in the body see same-iteration writes, each iteration gets
// a fresh binding, and the step operates on the next iteration's
// seed (invisible to already-captured bindings).
const fns: (() => string)[] = [];
for (let s = "a"; fns.length < 3; s = s + "b") {
  fns.push(() => s);
  s = s + "!";
}
console.log(fns[0](), fns[1](), fns[2]());
// break mid-iteration releases the live box
const got: (() => string)[] = [];
for (let t = "x"; ; t = t + "y") {
  got.push(() => t);
  if (t.length > 2) break;
  t = t + "z";
}
console.log(got.length, got[0](), got[got.length - 1]());
// array-typed for-init binding, same rewrite
const reads: (() => number)[] = [];
for (let xs: number[] = [1]; reads.length < 2; xs = [1, 2, 3]) {
  reads.push(() => xs.length);
  xs = xs.concat([9]);
}
console.log(reads[0](), reads[1]());
// never-written non-Copy for-init capture keeps the snapshot path
const snap: (() => string)[] = [];
for (let k = "fixed"; snap.length < 2; ) {
  snap.push(() => k);
}
console.log(snap[0](), snap[1]());
