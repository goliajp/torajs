// RFC 20260824-s2-5 刀 4 A2/A3 — the typed join kernel's exotic
// branch and the species guard's props probe sit behind link seams
// that a program which never makes an array exotic / never grows a
// props bag links as loud-reject stubs. This program DOES both, so
// both seams must stay bound to their real slow paths: an accessor
// index joins through the getter, and a poisoned `constructor`
// expando makes map throw TypeError (§9.4.2.3 step 7). The `as any`
// casts keep the bindings on the typed lane, so it is the TYPED
// kernels' exotic branches (`arr_join_i64`, `_locale`, the species
// guard) that cross the seams — the any-lane join has its own walk.
const xs: number[] = [1, 2, 3];
Object.defineProperty(xs as any, "1", {
  get: function (): number {
    return 42;
  },
});
console.log(xs.join(","));
console.log(xs.toLocaleString());
const ys: number[] = [4, 5, 6];
Object.defineProperty(ys as any, "constructor", { value: null });
try {
  const zs = ys.map((y: number) => y * 2);
  console.log("no-throw", zs.length);
} catch (e) {
  console.log("species:", e instanceof TypeError);
}
const plain: number[] = [7, 8, 9];
console.log(plain.map((p: number) => p + 1).join("-"));
