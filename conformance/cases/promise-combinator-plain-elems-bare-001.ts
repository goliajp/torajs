// §27.2.4.1.3 step 6.i resolve-wraps plain elements, so the BARE
// spelling `Promise.all([1, 2, 3])` — no `as any` — is legal for any
// element type; the checker admits it onto the dyn road and the
// result is any-shaped. A patched `Promise.resolve` then observes
// every element of such a run (t262
// invoke-resolve-*-every-iteration-of-promise).
async function main() {
  const a: any = await Promise.all([1, 2, 3]);
  console.log("all", a.length, a[0] + a[2]);
  const r: any = await Promise.race([7, 8]);
  console.log("race", r);
  const s: any = await Promise.allSettled(["x"]);
  console.log("allSettled", s[0].status, s[0].value);
  const w: any = await Promise.any([10]);
  console.log("any", w);
  let count = 0;
  const bound = Promise.resolve.bind(Promise);
  (Promise as any).resolve = function (...args: any[]) {
    count += 1;
    return bound(...args);
  };
  const b: any = await Promise.all([1, 1, 1]);
  console.log("patched", count, b.length);
}
main();
