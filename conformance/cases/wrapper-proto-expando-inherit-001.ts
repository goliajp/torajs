// Primitive-wrapper prototype expando inheritance (defineProperty
// 15.2.3.6-3-14x/16x wrapper families). A monkey-patch on
// String/Number/Boolean.prototype lands in the tag-3/0/4 singleton;
// wrapper receivers read through it, and §6.2.6.5
// ToPropertyDescriptor climbs the same chain when the descriptor is
// a wrapper object.

(String.prototype as any).value = "String";
const strObj: any = new String("abc");
console.log(strObj.value); // String (inherited)

const obj: any = {};
Object.defineProperty(obj, "property", strObj);
console.log(obj.property); // String

// writable through the chain too
(String.prototype as any).writable = true;
const obj2: any = {};
Object.defineProperty(obj2, "p2", new String("x") as any);
obj2.p2 = "isWritable";
console.log(obj2.p2); // isWritable

// own expando wins over the inherited one
const s2: any = new String("y");
s2.value = "own";
console.log(s2.value); // own

// Number/Boolean wrappers ride their own prototypes
(Number.prototype as any).mark = 1;
console.log((new Number(3) as any).mark); // 1
(Boolean.prototype as any).mark = 2;
console.log((new Boolean(true) as any).mark); // 2

// cleanup
delete (String.prototype as any).value;
delete (String.prototype as any).writable;
delete (Number.prototype as any).mark;
delete (Boolean.prototype as any).mark;
console.log(strObj.value); // undefined
console.log("done");
