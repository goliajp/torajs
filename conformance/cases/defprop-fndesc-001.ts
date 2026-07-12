// RFC 20260713-defprop-tpd-cluster chunk B — a descriptor argument
// that is not a plain object reads its fields through its own
// dynobj-backed store: Closure / Arr descriptors carry expando props
// at +24. Pre-fix the desc pointer was probed as a dynobj — SIGSEGV
// (test262 15.2.3.7-5-b-19/-20/-151 family); a typed (non-any)
// closure/array desc was also declined at lower time (silent no-op).

// typed closure descriptor with expando fields
var obj: any = {};
var descObj = function () {};
descObj.enumerable = true;
Object.defineProperties(obj, { prop: descObj });
var accessed = false;
for (var property in obj) {
  if (property === "prop") accessed = true;
}
console.log("fn-desc accessed:", accessed);

// data fields off a function descriptor
var obj2: any = {};
var func: any = function (a: any, b: any) {
  return a + b;
};
func.writable = false;
func.value = 42;
Object.defineProperties(obj2, { property: func });
var d2: any = Object.getOwnPropertyDescriptor(obj2, "property");
console.log("fn-desc fields:", d2.value, d2.writable, d2.enumerable, d2.configurable);

// fresh closure (no expando yet) — empty descriptor still creates
var obj3: any = {};
var fresh: any = function () {};
Object.defineProperties(obj3, { p: fresh });
var d3: any = Object.getOwnPropertyDescriptor(obj3, "p");
console.log("empty-fn-desc:", obj3.hasOwnProperty("p"), d3.writable, d3.enumerable, d3.configurable, d3.value);

// array descriptor with expando fields
var obj4: any = {};
var arrDesc = [];
arrDesc.enumerable = true;
Object.defineProperties(obj4, { prop: arrDesc });
var accessed4 = false;
for (var property4 in obj4) {
  if (property4 === "prop") accessed4 = true;
}
console.log("arr-desc accessed:", accessed4);
