// RFC 20260809 刀 1 — `using` declarations (sync form, Explicit
// Resource Management): reverse-order disposal, null/undefined skip,
// SuppressedError aggregation on double throw, return-through-
// dispose, bind-time method read (once), non-disposable TypeError,
// for-init loop-exit timing, nested block scopes, break-through-
// dispose, and generator early-return disposal.
const log: string[] = [];
function mk(tag: string): any {
  return { [Symbol.dispose]() { log.push(tag); } };
}
{
  using a = mk("a"), b = mk("b");
  using c = mk("c");
  log.push("body");
}
console.log(log.join(","));

{
  using n = null;
  using u = undefined;
  log.push("nu-ok");
}
console.log(log.length);

function f(): void {
  using x = { [Symbol.dispose]() { throw new Error("dispose-err"); } };
  throw new Error("body-err");
}
try { f(); } catch (e: any) {
  console.log(e.name, e.error.message, e.suppressed.message);
}

function g(): number {
  using r = mk("g-dispose");
  return 42;
}
console.log(g(), log[log.length - 1]);

let reads = 0;
const tricky: any = {};
Object.defineProperty(tricky, Symbol.dispose, {
  get() { reads = reads + 1; return function() { log.push("t"); }; }
});
{
  using t = tricky;
}
console.log("reads:", reads, log[log.length - 1]);

try {
  using bad = { notDispose: 1 } as any;
  console.log("unreachable");
} catch (e: any) { console.log("bind:", e.name); }

const l2: string[] = [];
for (using r = { [Symbol.dispose]() { l2.push("for-d"); } } as any; false; ) {}
console.log("for:", l2.join(","));

const l3: string[] = [];
function mk3(t: string): any { return { [Symbol.dispose]() { l3.push(t); } }; }
{
  using o = mk3("outer");
  { using i2 = mk3("inner"); }
  l3.push("mid");
}
console.log("nest:", l3.join(","));

while (true) {
  using w = mk3("w");
  break;
}
console.log("break:", l3.join(","));

const l5: string[] = [];
function* gen() {
  using gr = { [Symbol.dispose]() { l5.push("gen-d"); } } as any;
  yield 1;
  yield 2;
}
const it = gen();
it.next();
it.return(0);
console.log("gen:", l5.join(","));
