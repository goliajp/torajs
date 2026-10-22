// On a promise of `any`, a handler whose parameter takes every value
// (unannotated, `any`, or a union) never runs an untyped twin, so the
// leg settles what the handler declares or returns.

async function main() {
  const x: any = "s";
  const a = await Promise.resolve(x).then((v) => 1, () => 0);
  console.log("bare", typeof a, a + 1);
  const b = await Promise.resolve(x).then((v: any) => v.length, () => 0);
  console.log("any", typeof b, b);
  const c = await Promise.resolve(x).then((v: string | number) => typeof v, () => "e");
  console.log("union", typeof c, c);
  const n: any = 3;
  const d = await Promise.resolve(n).then((v) => v * 2, () => 0);
  console.log("num", typeof d, d);
}
main();
