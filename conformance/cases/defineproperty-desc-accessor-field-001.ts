// ToPropertyDescriptor [[Get]] semantics — a descriptor field that is
// itself an accessor property invokes its getter (test262
// 15.2.3.6-3-215/-245 / 15.2.3.5-4-56 / 15.2.3.7-5-b-201 shapes)
// "get" field is an accessor with only a setter -> [[Get]] undefined
const attributes: any = {};
Object.defineProperty(attributes, "get", { set: () => {} });
const obj: any = {};
Object.defineProperty(obj, "property", attributes);
console.log(typeof obj.property, Object.prototype.hasOwnProperty.call(obj, "property"));
// "value" field is a getter -> must be invoked
let accessed = false;
const attr2: any = {};
Object.defineProperty(attr2, "value", { get: () => { accessed = true; return 5; } });
const o2: any = {};
Object.defineProperty(o2, "p", attr2);
console.log(o2.p, accessed);
// "enumerable" field is a getter
let eAccessed = false;
const attr3: any = {};
Object.defineProperty(attr3, "enumerable", { get: () => { eAccessed = true; return true; } });
const o3: any = {};
Object.defineProperty(o3, "e", attr3);
console.log(eAccessed, Object.keys(o3).length);
// "get" field is a getter answering a closure
const attr4: any = {};
Object.defineProperty(attr4, "get", { get: () => () => 42 });
const o4: any = {};
Object.defineProperty(o4, "g", attr4);
console.log(o4.g);
// same through defineProperties (15.2.3.7-5-b-201 shape)
const d5: any = {};
Object.defineProperty(d5, "get", { set: () => {} });
const props: any = { p5: d5 };
const o5: any = {};
Object.defineProperties(o5, props);
console.log(typeof o5.p5, Object.prototype.hasOwnProperty.call(o5, "p5"));
// plain data descriptors keep working
const o6: any = {};
const dd: any = { value: 7, writable: true, enumerable: true };
Object.defineProperty(o6, "v", dd);
console.log(o6.v);
