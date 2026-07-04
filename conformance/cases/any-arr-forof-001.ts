// Any-dynamic-access RFC (20260704) S5 — for..of over an `any`
// receiver drives the same i-loop as Array<T>, reading elements
// through the S3 runtime index dispatch and sizing via
// __torajs_any_iter_len (catchable TypeError on non-iterables).
const a: any = [1, 2];
for (const x of a) {
  console.log(x);
}
const t: number[] = [7, 8];
const b: any = t;
for (const y of b) {
  console.log(y);
}
const ss: string[] = ["p", "q"];
const c: any = ss;
for (const z of c) {
  console.log(z);
}
const s: any = "hi";
for (const ch of s) {
  console.log(ch);
}
try {
  const n: any = 5;
  for (const w of n) {
    console.log(w);
  }
} catch (err) {
  console.log("not iterable caught");
}
