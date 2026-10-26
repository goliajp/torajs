// An await inside a destructuring assignment target or default: the
// default and the target reference suspend where the pattern evaluates
// them, interleaved with other jobs in spec order.

const log: string[] = [];
function tick(n: number) {
  let p: Promise<void> = Promise.resolve();
  for (let i = 0; i < n; i++) p = p.then(() => { log.push("t" + i) });
}
async function* g() {
  let a = 0, b = 0;
  const o: any = {};
  [a = await Promise.resolve(7), b] = [undefined, 2] as any;
  log.push("ab" + a + b);
  yield a + b;
  ({ k: o[await Promise.resolve("key")], m: b = await Promise.resolve(9) } = { k: 5 } as any);
  log.push("o" + o.key + b);
  yield o.key + b;
}
async function f() {
  let a = 0, b = 0;
  [a = await Promise.resolve(3), b] = [undefined, 4] as any;
  log.push("f" + a + b);
}
async function main() {
  tick(6);
  for await (const v of g()) log.push("v" + v);
  log.push("|");
  tick(3);
  await f();
  console.log(log.join(" "));
}
main();
