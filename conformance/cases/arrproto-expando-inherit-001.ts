// Array.prototype expando inheritance (defineProperty
// 15.2.3.6-3-87-1 shape). Array.prototype is the tag-2 builtin
// singleton — an Arr cell whose monkey-patches land in its props
// dynobj; array receivers and array-shaped descriptors inherit
// through it.

(Array.prototype as any).configurable = true;

// arr member read inherits
const arrObj: any = [1, 2, 3];
console.log(arrObj.configurable); // true

// own expando wins
const a2: any = [4];
a2.configurable = false;
console.log(a2.configurable); // false

// ToPropertyDescriptor climbs to Array.prototype: the property
// comes out configurable, so delete works
const obj: any = {};
Object.defineProperty(obj, "property", arrObj);
console.log(obj.hasOwnProperty("property")); // true
delete obj.property;
console.log(obj.hasOwnProperty("property")); // false

// cleanup — later cases must not see the patch
delete (Array.prototype as any).configurable;
console.log(arrObj.configurable); // undefined
console.log("done");
