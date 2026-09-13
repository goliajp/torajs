// RFC 20260805 D1b — an `await` inside an expression suspends too: the
// lift hoists it to statement level ahead of the statement, and every
// operand evaluated before it is bound first, so the statement reads
// the values it had produced before the suspension. Each function
// here suspends at its first await, so bun's interleaving holds.
const log: string[] = [];
const tick = (tag: string) => Promise.resolve().then(() => { log.push(tag); });
let order = "";
async function argPosition() {
  const a = (async () => { await Promise.resolve(1); order += "a"; return 1; })();
  order += "s";
  console.log("arg", order, await a, order);
}
async function returnAwait(): Promise<number> {
  return await Promise.resolve(41).then((v) => v + 1);
}
async function nestedCalls() {
  const seen: string[] = [];
  const f = (x: any, y: any) => { seen.push("f"); return [x, y]; };
  const g = (x: any) => { seen.push("g:" + x); return x * 2; };
  const r = f(g(await Promise.resolve(3)), await Promise.resolve(4));
  console.log("nested", seen.join(","), r);
}
async function memberTarget() {
  let o = { x: 0 };
  const first = o;
  const p = Promise.resolve(7).then((v) => { o = { x: -1 }; return v; });
  o.x = await p;
  console.log("target", first.x, o.x);
}
async function compound() {
  let n = 1;
  const p = Promise.resolve(10).then((v) => { n = 100; return v; });
  n += await p;
  console.log("compound", n);
}
async function binop() {
  const v = 1 + (await Promise.resolve(2)) * (await Promise.resolve(3));
  console.log("binop", v);
}
async function condition() {
  if (await Promise.resolve(true)) { console.log("cond", "yes"); }
  const s = (await Promise.resolve("k")) === "k" ? "eq" : "ne";
  console.log("ternary-cond", s);
}
async function methodCall() {
  const xs: number[] = [1, 2, 3];
  console.log("method", xs.map((x) => x * (2 as number)).join(","), await Promise.resolve("z"));
  console.log("spread", Math.max(...xs, await Promise.resolve(9)));
}
async function thenable() {
  const t = { then(resolve: any, reject: any) { resolve(7); } };
  console.log("thenable", "v=" + (await t));
}
async function guarded() {
  // the guarded side of `&&` is not lifted: this function keeps the
  // synchronous lowering, and still answers right
  const a = true;
  console.log("guarded", a && (await Promise.resolve("both")));
}
async function main() {
  await argPosition();
  await tick("t1");
  console.log("return", await returnAwait());
  await nestedCalls();
  await memberTarget();
  await compound();
  await binop();
  await condition();
  await methodCall();
  await thenable();
  await guarded();
  console.log("log", log.join(","));
}
main();
