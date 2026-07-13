// RFC 20260713-accessor-void-kind blade 1 — a throw-only / no-return
// getter lowers to a native void fn; the invoke path must not read
// its return register (pre-fix: x0 garbage → SIGSEGV in
// value_drop_heap under the Array generic arm, silent garbage on a
// plain read).

// throwing length getter under an Array generic method — the getter's
// exception propagates (test262 15.4.4.16-4-10 family shape)
var obj: any = { 0: 11, 1: 12 };
Object.defineProperty(obj, "length", {
  get: function () {
    throw new Error("boom");
  },
  configurable: true,
});
try {
  Array.prototype.every.call(obj, undefined);
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}
try {
  Array.prototype.reduce.call(obj, function () {});
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}

// void getter (side effect, no return) — a plain read answers
// undefined, and the side effect runs exactly once per read
var side: any = {};
let hits = 0;
Object.defineProperty(side, "probe", {
  get: function () {
    hits++;
  },
  configurable: true,
});
console.log("probe =", side.probe);
console.log("hits =", hits);

// conditional-throw void getter — the non-throw path still answers
// undefined
var cond: any = {};
let arm = false;
Object.defineProperty(cond, "v", {
  get: function () {
    if (arm) throw new Error("armed");
  },
  configurable: true,
});
console.log("v =", cond.v);
arm = true;
try {
  cond.v;
  console.log("no throw");
} catch (e) {
  console.log("caught:", (e as Error).message);
}
