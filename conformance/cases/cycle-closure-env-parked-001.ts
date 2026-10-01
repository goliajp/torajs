// A closure env that was buffered as a cycle candidate (a release
// through the any lane left it at rc > 0 while it could trace a
// capture) and then died before the next collect stays allocated for
// the collector's root walk, which must free it at the env's own
// block size: nothing in front of the block records one, and an env
// has no layout row. The typed closure drop never buffers, so the
// shares are held and released through `any` slots. 300k such envs
// pass through 290-odd collects; a free at the wrong size corrupts the
// allocator's size classes long before the loop ends.
function make(i: number): () => number {
  const o = { v: i };
  return () => o.v;
}
let sum = 0;
for (let i = 0; i < 300000; i++) {
  const f = make(i);
  const a: any = f;
  const b: any = f;
  sum += a() + b();
}
console.log(sum);
console.log("done");
