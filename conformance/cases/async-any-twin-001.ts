type P = { a: number };
async function af(s: string) {
  await 0;
  return s.length;
}
async function both(p: P, tag: string) {
  const v = await Promise.resolve(p.a);
  return tag + ":" + v;
}
async function noAwait(s: string) {
  return s.length;
}
const f: any = af;
const b: any = both;
const na: any = noAwait;
const arrow: any = async (s: string) => {
  await null;
  return typeof s + s.length;
};
const x: any = 7;
async function main() {
  console.log(await f(5), await f("abc"), await f(null).catch((e: any) => e instanceof TypeError));
  console.log(await b({ a: 3 }, "t"), await b(4, 5), await b({ a: 1, z: 2 }, null));
  console.log(await na(5), await na("abcd"));
  console.log(await arrow(5), await arrow("xy"));
  console.log(await af(x), await af("abcde"));
}
main();
