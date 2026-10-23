// Promise.all at top level over an `any` array cast to
// `Promise<number>[]`, and over the same array handed to a typed
// parameter: the result holds what each element settles with (a plain
// value, a thenable's argument), whatever the cast says.
const a: any = [Promise.resolve(1), 2, { then(r: any) { r(3); } }];
const b = a as Promise<number>[];
const r = await Promise.all(b);
console.log(r.length, r[0], r[1], r[2]);

function src(): any {
  return [Promise.resolve(10), 20, { then(r: any) { r(30); } }];
}
async function sum(xs: Promise<number>[]): Promise<number> {
  const vs = await Promise.all(xs);
  let s = 0;
  for (const v of vs) s += v;
  return s;
}
async function main() {
  const typed: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2)];
  console.log(await sum(typed));
  console.log(await sum(src()));
  const held: any = src();
  const viaVar: Promise<number>[] = held;
  const rv = await Promise.all(viaVar);
  console.log(rv[0] + rv[1] + rv[2]);
}
await main();
