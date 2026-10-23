// An array an async function returns settles its promise as the
// array, readable from a handler that takes the value as `any`
// (the element kind is marked, as `Promise.resolve` marks it) and
// from a typed await.
type A = { right: number };
async function structs(): Promise<A[]> {
  await null;
  return [{ right: 0.25 }];
}
async function nums(): Promise<number[]> {
  await null;
  return [1.5, 2];
}
async function ints(n: number): Promise<number[]> {
  await null;
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 3);
  return out;
}
async function strs(): Promise<string[]> {
  await 0;
  return ["a", "b" + nums.length];
}
async function nested(): Promise<number[][]> {
  await null;
  return [[0.5], [1, 2]];
}
async function flags(): Promise<boolean[]> {
  await null;
  return [true, false];
}
structs().then((v) => console.log("structs", v[0].right, v.length));
nums().then((v) => console.log("nums", v[0], v[1], v.length));
ints(4).then((v) => console.log("ints", v[3], v.join("-")));
strs().then((v) => console.log("strs", v[0], v[1]));
nested().then((v) => console.log("nested", v[0][0], v[1][1]));
flags().then((v) => console.log("flags", v[0], v[1]));
async function main() {
  const a = await nums();
  const b = await nested();
  console.log("typed", a[0] + a[1], b[1][0] + b[0][0]);
}
main();
