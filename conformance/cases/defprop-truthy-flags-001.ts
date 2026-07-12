// RFC 20260713-defprop-tpd-cluster chunk A — a literal descriptor
// whose flag field is not a Bool literal must take §6.2.6.5 ToBoolean
// semantics (present + truthy), not silent absent-treatment.
// Pre-fix: `enumerable: -9` set no present bit — the property landed
// non-enumerable (test262 15.2.3.7-5-b-4x family).

// dynobj receiver — truthy number flag
var obj: any = {};
Object.defineProperties(obj, { prop: { enumerable: -9 } });
var accessed = false;
for (var property in obj) {
  if (property === "prop") accessed = true;
}
console.log("accessed:", accessed);
var d0: any = Object.getOwnPropertyDescriptor(obj, "prop");
console.log("enum:", d0.enumerable, "writ:", d0.writable, "conf:", d0.configurable);

// falsy non-Bool flag — present + false
var obj2: any = {};
Object.defineProperties(obj2, { p: { value: 7, enumerable: 0, writable: "", configurable: true } });
var d2: any = Object.getOwnPropertyDescriptor(obj2, "p");
console.log("p:", d2.value, d2.writable, d2.enumerable, d2.configurable);

// Arr receiver — truthy string/number flags route through the same
// runtime fallback (define_apply's TAG_ARR dispatch)
let arr = [];
Object.defineProperties(arr, {
  "0": { value: 100, writable: 1, enumerable: "yes", configurable: true },
});
var d1: any = Object.getOwnPropertyDescriptor(arr, "0");
console.log("arr:", d1.value, d1.writable, d1.enumerable, d1.configurable, arr[0], arr.length);

// defineProperty single-key shape shares emit_define_one
var obj3: any = {};
Object.defineProperty(obj3, "k", { value: 3, configurable: 1 });
var d3: any = Object.getOwnPropertyDescriptor(obj3, "k");
console.log("k:", d3.value, d3.writable, d3.enumerable, d3.configurable);
