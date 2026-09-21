// §20.1.3.6 step 6 — `Object.prototype.toString`'s builtinTag is
// "Function" exactly when the receiver has [[Call]], i.e. when
// IsCallable says so. tr spelled the shapes out again at this site
// and named two of the three: a closure cell, and a class object
// (a dynobj carrying [[Call]]). The callable Proxy fell past every
// arm to the "Object" default.
//
// Step 4's Array is decided above this one and steps 11-12's Date /
// RegExp below it, so asking §7.2.3 here keeps the walk in spec
// order.
function attempt(f: any): string {
  try {
    return "ran:" + String(f());
  } catch (e: any) {
    return "threw:" + (e && e.constructor ? e.constructor.name : "?");
  }
}
const tag = (v: any): string => Object.prototype.toString.call(v);
function add(a: number, b: number): number {
  return a + b;
}
class K {}
console.log("proxy of fn    ", attempt(() => tag(new Proxy(add as any, {}))));
console.log("proxy of class ", attempt(() => tag(new Proxy(K as any, {}))));
console.log("proxy of arrow ", attempt(() => tag(new Proxy(((x: number) => x) as any, {}))));
console.log("proxy w/ trap  ", attempt(() =>
  tag(new Proxy(add as any, { get: (t: any, k: any) => Reflect.get(t, k) }))));
console.log("proxy of bound ", attempt(() => tag(new Proxy(add.bind(null) as any, {}))));
// Not callable — the default still wins.
console.log("proxy of object", attempt(() => tag(new Proxy({ a: 1 } as any, {}))));
// The two shapes the site already knew, unmoved.
console.log("plain fn       ", attempt(() => tag(add)));
console.log("class object   ", attempt(() => tag(K)));
console.log("builtin method ", attempt(() => tag(Array.prototype.map)));
console.log("intrinsic ctor ", attempt(() => tag(Error)));
// And the arms on either side of step 6 keep their own answers.
console.log("array          ", attempt(() => tag([1, 2])));
console.log("date           ", attempt(() => tag(new Date(0))));
console.log("regexp         ", attempt(() => tag(/x/)));
console.log("plain object   ", attempt(() => tag({})));
console.log("error          ", attempt(() => tag(new Error("e"))));
console.log("string         ", attempt(() => tag("s")));
console.log("number         ", attempt(() => tag(1)));
