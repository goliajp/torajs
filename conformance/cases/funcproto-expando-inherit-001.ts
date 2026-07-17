// Function.prototype expando inheritance (defineProperty
// 15.2.3.6-3-16x family). A monkey-patch on Function.prototype
// lands in the tag-13 builtin-proto singleton dynobj; every closure
// reads it through the member lanes, and §6.2.6.5
// ToPropertyDescriptor's [[Get]] climbs the same chain when the
// descriptor is a function.

(Function.prototype as any).writable = true;

// closure member read inherits
const funObj: any = function (a: any, b: any) { return a + b; };
console.log(funObj.writable); // true

// own expando still wins over the inherited one
const g: any = () => 0;
g.writable = false;
console.log(g.writable); // false

// ToPropertyDescriptor reads the inherited field: obj.property
// becomes writable
const obj: any = {};
Object.defineProperty(obj, "property", funObj);
console.log(obj.hasOwnProperty("property")); // true
obj.property = "isWritable";
console.log(obj.property); // isWritable

// cleanup — later cases must not see the patch
delete (Function.prototype as any).writable;
console.log(funObj.writable); // undefined
console.log("done");
