// Top-level awaits of async calls that may run a specialization: an
// `any` array argument sends `sum` to its all-`any` twin, whose result
// comes back in any form, while the typed calls keep the direct read of
// the frame's result.
function src(): any {
  return [Promise.resolve(10), 20, { then(r: any) { r(30); } }];
}
async function sum(xs: Promise<number>[]): Promise<number> {
  const vs = await Promise.all(xs);
  let s = 0;
  for (const v of vs) s += v;
  return s;
}
async function half(n: number): Promise<number> {
  await null;
  return n / 2;
}
async function tag(n: number): Promise<string> {
  await null;
  return "t" + n;
}
function anyNum(): any { return "7"; }
console.log(await sum(src()));
console.log(await sum([Promise.resolve(1), Promise.resolve(2)]));
console.log(await half(anyNum()));
console.log(await half(9));
console.log(await tag(anyNum()));
const r = await sum(src() as Promise<number>[]);
console.log(r + 1, typeof r);
console.log(await (0, sum(src())));
