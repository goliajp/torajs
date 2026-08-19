// §27.2.4.1.3 step 6.i over PLAIN elements — a patched
// `Promise.resolve` is invoked once per raw scalar element of a
// value-kind array (the as-cast road), with the element as its
// argument; the pre-fix sync consult re-boxed each scalar as a heap
// cell (SIGSEGV).
var count = 0;
Promise.resolve = function (v: any) {
  count++;
  return new Promise(function (res: any) { res(v * 10); });
};
async function main() {
  const a: any = await Promise.all([1, 2] as any);
  console.log("all", a[0], a[1], "count", count);
}
main();
