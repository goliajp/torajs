// Chunk 698 — an Array<Any> value bound to a typed Array<T>
// annotation decode-copies into a fresh typed block at the assign
// boundary (`const a: number[] = Array.from(set)` previously bound
// the NaN-box block to a typed binding and the raw-slot readers
// misdecoded box bits: [10, 20] printed as -562949953421302, …).
// Covers both the general path and the K.3/K.4 top-level global
// lane. Owned-shape inits only (a fresh value has no other
// reference, so the copy is unobservable); a borrow-shape init
// (`const t: number[] = src`) is a JS reference alias and keeps
// the pre-698 behavior (L3b). Mismatched elements throw a
// catchable TypeError before anything allocates.
const s = new Set<number>();
s.add(10);
s.add(20);
const a: number[] = Array.from(s);
console.log(a);
console.log(a[0] + a[1]);
// string elems (heap kind — the fresh block owns its shares)
const ss = new Set<string>();
ss.add("hi");
ss.add("yo");
const b: string[] = Array.from(ss);
console.log(b);
console.log(b[0].length + b[1].length);
// fn-scope (general path)
function go(): number {
  const fs = new Set<number>();
  fs.add(7);
  const fa: number[] = Array.from(fs);
  return fa[0];
}
console.log(go());
