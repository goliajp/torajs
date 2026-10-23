// a `using` declaration does not move the bindings declared after it
// out of reach of what the same block declares or writes before it:
// a hoisted function or an arrow written ahead still sees them, and
// a call of either before the declaration still meets the dead zone
function hoisted(): void {
  function f() { return (x as any) === null ? "null" : "other"; }
  try { f(); } catch (e: any) { console.log("a1", e.constructor.name); }
  using x = null;
  console.log("a2", f());
}
function arrowAhead(): void {
  const g = () => y;
  try { g(); } catch (e: any) { console.log("b1", e.constructor.name); }
  using r = { [Symbol.dispose]() { console.log("b disposed"); } };
  const y = 5;
  console.log("b2", g());
}
function laterLet(): void {
  function h() { return z * 2; }
  using r = { [Symbol.dispose]() { console.log("c disposed"); } };
  let z = 21;
  console.log("c1", h());
}
function blockLevel(): string {
  const out: string[] = [];
  {
    const read = () => w;
    using r = { [Symbol.dispose]() { out.push("disposed"); } };
    const w = "w";
    out.push(read());
  }
  return out.join(",");
}
// statements ahead of the first resource leave the block as they
// would without it: a throw keeps its value, a return keeps its own
function earlyThrow(): void {
  const err = new Error("early");
  try {
    (() => { throw err; })();
    using r = null;
  } catch (e) {
    console.log("d1", e === err);
  }
}
function earlyReturn(flag: boolean): string {
  if (flag) return "early";
  using r = { [Symbol.dispose]() { console.log("e disposed"); } };
  return "late";
}
async function asyncAhead(): Promise<void> {
  function f() { return x === null; }
  try { f(); } catch (e: any) { console.log("f1", e.constructor.name); }
  await using x = null;
  console.log("f2", f());
}
async function asyncEarly(flag: boolean): Promise<string> {
  if (flag) return "early";
  await using r = { async [Symbol.asyncDispose]() { console.log("g disposed"); } };
  return "late";
}
hoisted();
arrowAhead();
laterLet();
console.log(blockLevel());
earlyThrow();
console.log(earlyReturn(true), earlyReturn(false));
(async () => {
  await asyncAhead();
  // an early return before any resource awaits nothing on the way
  // out, so it settles ahead of a promise queued after the call
  const order: string[] = [];
  const p = asyncEarly(true).then((v) => { order.push("a:" + v); });
  Promise.resolve().then(() => { order.push("tick1"); });
  Promise.resolve().then(() => {}).then(() => { order.push("tick2"); });
  await p;
  await null;
  await null;
  console.log(order.join(","));
  console.log(await asyncEarly(false));
})();
