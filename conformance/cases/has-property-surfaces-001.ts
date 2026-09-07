// §28.1.9 `Reflect.has(target, key)` is §13.10.1 `key in target`
// verbatim — same three steps in the same order. It used to be
// emitted as a copy of `Object.hasOwn`'s own-property probe instead.

const o = { a: 1 };

// The chain. `hasOwn` is own-only (§20.1.2.4) and must NOT walk it.
console.log(Reflect.has(o, "a"), Reflect.has(o, "toString"), Reflect.has(o, "b"));
console.log(Object.hasOwn(o, "a"), Object.hasOwn(o, "toString"), Object.hasOwn(o, "b"));

const m = new Map<string, number>();
(m as any).tag = 1;
console.log(Reflect.has(m as any, "get"), Reflect.has(m as any, "size"));
console.log(Reflect.has(m as any, "tag"), Reflect.has(m as any, "nope"));
console.log(Object.hasOwn(m as any, "get"), Object.hasOwn(m as any, "tag"));

// Step 1: a non-Object target throws, it does not answer false.
try {
  Reflect.has(42 as any, "x");
  console.log("no throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}
// §20.1.2.4 step 1 is ToObject, not a type test — a primitive there
// has an own face and answers.
console.log(Object.hasOwn(42 as any, "x"), Object.hasOwn("abc" as any, "0"));
console.log(Object.hasOwn("abc" as any, "9"), Object.hasOwn("abc" as any, "length"));

// A statically known receiver shape reaches the same face. Each of
// these died at "unsupported member call shape" while the identical
// value spelled `any` answered.
const a = [1, 2, 3];
console.log(Object.hasOwn(a as any, "0"), Object.hasOwn(a as any, "9"));
console.log(Object.hasOwn(a as any, "length"), Object.hasOwn(a as any, "map"));
console.log(Reflect.has(a as any, 0), Reflect.has(a as any, 9), Reflect.has(a as any, "map"));

function g() {
  return 1;
}
console.log(Object.hasOwn(g as any, "name"), Object.hasOwn(g as any, "call"));
console.log(Reflect.has(g as any, "name"), Reflect.has(g as any, "call"));

const wm = new WeakMap<object, number>();
const wr = new WeakRef<object>({});
console.log(Reflect.has(wm as any, "get"), Reflect.has(wr as any, "deref"));
console.log(Object.hasOwn(wm as any, "get"));

console.log(Reflect.has(new Date(0) as any, "getTime"), Reflect.has(/a/g as any, "test"));
console.log(Reflect.has([1].values() as any, "next"));

// §7.1.19 answers the whole key domain, not a list of admitted kinds.
const k: any = {};
k["true"] = 1;
console.log(Reflect.has(k, true as any), true in k, Object.hasOwn(k, true as any));
const dyn = "ta" + "g";
console.log(Reflect.has(m as any, dyn), Object.hasOwn(m as any, dyn));
const sym = Symbol("s");
const so: any = {};
so[sym] = 1;
console.log(Reflect.has(so, sym), Object.hasOwn(so, sym));

// Argument order: `Reflect.has(o, k)` evaluates the object first,
// `k in o` the key first. The shared emit takes them lowered.
function fo(): any {
  console.log("obj");
  return { a: 1 };
}
function fk(): string {
  console.log("key");
  return "a";
}
console.log(Reflect.has(fo(), fk()));
console.log(Object.hasOwn(fo(), fk()));

// Trailing arguments evaluate and are discarded.
let n = 0;
function side(): number {
  n += 1;
  return n;
}
console.log(Reflect.has(o as any, "a", side() as any), n);
