// §7.3.12 HasOwnProperty and §20.1.3.4 propertyIsEnumerable reach a
// Proxy through [[GetOwnProperty]]; `in` and Reflect.has through
// [[HasProperty]]. Each spelling fires exactly one trap.
const log: string[] = [];
const target: any = { own: 1 };
Object.defineProperty(target, "hidden", { value: 2, enumerable: false, configurable: true });
const p: any = new Proxy(target, {
  has(t: any, k: any) {
    log.push("has:" + String(k));
    return Reflect.has(t, k);
  },
  getOwnPropertyDescriptor(t: any, k: any) {
    log.push("gopd:" + String(k));
    return Reflect.getOwnPropertyDescriptor(t, k);
  },
});
const hop = Object.prototype.hasOwnProperty;
const pie = Object.prototype.propertyIsEnumerable;
function step(label: string, f: () => unknown) {
  log.length = 0;
  const v = f();
  console.log(label, v, log.join(","));
}
step("hasOwnProperty own", () => hop.call(p, "own"));
step("hasOwnProperty missing", () => hop.call(p, "toString"));
step("propertyIsEnumerable own", () => pie.call(p, "own"));
step("propertyIsEnumerable hidden", () => pie.call(p, "hidden"));
step("in inherited", () => "toString" in p);
step("in own", () => "own" in p);
step("Reflect.has inherited", () => Reflect.has(p, "toString"));
step("Object.hasOwn", () => Object.hasOwn(p, "own"));
