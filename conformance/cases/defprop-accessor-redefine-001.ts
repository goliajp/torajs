// RFC 20260713-defprop-tpd-cluster chunk D — accessor partial
// redefine + named-fn accessor faces. Pre-fix: (1) a named top-level
// fn used as get/set stored its raw code address as a closure cell —
// invoke/drop read code memory as a heap header, SIGBUS; (2) any
// redefine of an existing accessor entry crashed the same way, and
// a partial descriptor ({set: undefined} over {get,set}) had no
// §10.1.6.3 merge semantics (absent face keeps current, explicit
// undefined clears).

// named top-level fn as getter (zero-capture env mint, NAKED invoke)
var obj: any = {};
function g(): any {
  return 42;
}
Object.defineProperty(obj, "q", { get: g, configurable: true });
console.log("named getter:", obj.q);
var d: any = Object.getOwnPropertyDescriptor(obj, "q");
console.log("gopd get:", typeof d.get);

// redefine with the same getter — no crash, value unchanged
Object.defineProperties(obj, { q: { get: g } });
console.log("redefine same-get:", obj.q);

// partial redefine: set: undefined clears the setter, keeps the get
var obj2: any = {};
function get_func(): any {
  return 10;
}
function set_func(v: any): any {
  return 10;
}
Object.defineProperty(obj2, "property", {
  get: get_func,
  set: set_func,
  enumerable: true,
  configurable: true,
});
Object.defineProperties(obj2, { property: { set: undefined } });
console.log("get kept:", obj2.property);
var d2: any = Object.getOwnPropertyDescriptor(obj2, "property");
console.log("set cleared:", typeof d2.set === "undefined", "get kept:", typeof d2.get);

// fresh define with explicit set: undefined
var obj3: any = {};
Object.defineProperty(obj3, "p", { set: undefined });
var d3: any = Object.getOwnPropertyDescriptor(obj3, "p");
console.log("fresh:", obj3.hasOwnProperty("p"), typeof d3.set === "undefined");

// e/c fold across an accessor redefine keeps absent attributes
var d4: any = Object.getOwnPropertyDescriptor(obj2, "property");
console.log("attrs kept:", d4.enumerable, d4.configurable);
