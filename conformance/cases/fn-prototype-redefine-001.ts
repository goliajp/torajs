// §10.2.5 MakeConstructor — a function's own `prototype` is writable,
// non-enumerable and non-configurable from the start, so a define that
// would change its kind or attributes is refused even when it is the
// first touch of the property.
function kind(f: () => unknown): string {
  try {
    const v = f();
    return "ok " + String(v);
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other " + String(e);
  }
}
const f1: any = function () {};
console.log("setter", kind(() => Object.defineProperty(f1, "prototype", { set: function (_v: any) {} }) === f1));
console.log("after", JSON.stringify(Object.getOwnPropertyDescriptor(f1, "prototype")));
const f2: any = function () {};
console.log("enumerable", kind(() => Object.defineProperty(f2, "prototype", { enumerable: true }) === f2));
const f3: any = function () {};
console.log("configurable", Reflect.defineProperty(f3, "prototype", { configurable: true }));
const f4: any = function () {};
const p = { marker: 1 };
console.log("value", Reflect.defineProperty(f4, "prototype", { value: p }), f4.prototype === p);
console.log("new uses it", Object.getPrototypeOf(new f4()) === p);
const f5: any = function () {};
console.log("non-writable", Reflect.defineProperty(f5, "prototype", { writable: false }), JSON.stringify(Object.getOwnPropertyDescriptor(f5, "prototype")));
const arrow: any = () => 1;
console.log("arrow", Reflect.defineProperty(arrow, "prototype", { get() { return 7; }, configurable: true }), arrow.prototype);
