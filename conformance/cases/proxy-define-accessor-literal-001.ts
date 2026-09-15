// A literal accessor descriptor handed to a Proxy reaches the trap and
// the target with its get / set fields (§10.5.6 step 8 builds the trap
// argument from the whole descriptor).
const seen: string[] = [];
const target: any = {};
const p: any = new Proxy(target, {
  defineProperty(t: any, k: any, d: any) {
    seen.push(String(k) + ":" + Object.keys(d).sort().join("/") + ":" + typeof d.get + ":" + typeof d.set);
    return Reflect.defineProperty(t, k, d);
  },
});
Object.defineProperty(p, "a", { get() { return 41 + 1; }, configurable: true });
Object.defineProperty(p, "b", { set(_v: any) {}, enumerable: true });
Object.defineProperty(p, "c", { get: undefined, set: undefined });
console.log(seen.join(" | "));
console.log(target.a, JSON.stringify(Object.getOwnPropertyDescriptor(target, "b")), JSON.stringify(Object.getOwnPropertyDescriptor(target, "c")));

// A trap-less proxy forwards the accessor to the target, whose
// validation refuses it on a non-configurable data property.
const fn: any = function () {};
const fp: any = new Proxy(new Proxy(fn, {}), {});
try {
  Object.defineProperty(fp, "prototype", { set(_v: any) {} });
  console.log("accepted");
} catch (e: any) {
  console.log(e instanceof TypeError ? "TypeError" : "other");
}
const arr: any[] = [];
const ap: any = new Proxy(new Proxy(arr, {}), { defineProperty: undefined });
try {
  Object.defineProperty(ap, "length", { get() { return 0; } });
  console.log("accepted");
} catch (e: any) {
  console.log(e instanceof TypeError ? "TypeError" : "other " + e?.constructor?.name);
}
const fwd: any = {};
Object.defineProperty(new Proxy(fwd, {}), "g", { get() { return "via"; }, configurable: true });
console.log(fwd.g);
