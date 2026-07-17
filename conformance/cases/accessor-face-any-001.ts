// §6.2.6.5 steps 7-8 — an Any-typed accessor face unboxes through
// IsCallable before landing in the AccessorPair. Pre-fix the face
// slot stored the NaN-box bits verbatim and the property access
// transmuted them as a cell — SIGSEGV.

const getFun: any = function () { return 1; };
const o: any = {};
Object.defineProperty(o, "a", { get: getFun, enumerable: true });
console.log(o.a); // 1

// setter face through any (captures instead of this — the
// fn-expr this channel is its own RFC surface)
let stash = 0;
const setFun: any = function (v: any) { stash = v * 2; };
Object.defineProperty(o, "x", { set: setFun });
o.x = 21;
console.log(stash); // 42

// undefined face clears
const u: any = undefined;
Object.defineProperty(o, "e", { get: u, enumerable: true });
console.log(o.e); // undefined

// non-callable face throws
try {
  Object.defineProperty(o, "bad", { get: 42 as any });
  console.log("no throw");
} catch (e) {
  console.log("caught:", e instanceof TypeError);
}
console.log("done");
