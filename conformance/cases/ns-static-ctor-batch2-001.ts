// ctor statics batch 2 (RFC 20260720-ctor-static-reflection 刀 4)
// — the Object integrity family (getOwnPropertyNames /
// preventExtensions / isExtensible / seal / isSealed) as reified
// value cells: name/length reflection, real dispatch (nonenum keys
// walk, receiver identity, primitive pass-through semantics) and
// the gOPD ctor-static face riding the same interned identity.
const gopn: any = Object.getOwnPropertyNames;
console.log(gopn.name, gopn.length);
const o: any = {};
Object.defineProperty(o, "h", { value: 1, enumerable: false });
o.v = 2;
console.log(JSON.stringify(gopn(o)), JSON.stringify(Object.keys(o)));
const pe: any = Object.preventExtensions;
const ie: any = Object.isExtensible;
console.log(pe.name, pe.length, ie.name, ie.length);
const o2: any = {};
console.log(ie(o2), pe(o2) === o2, ie(o2));
const se: any = Object.seal;
const is: any = Object.isSealed;
console.log(se.name, is.name);
const o3: any = { a: 1 };
console.log(is(o3), se(o3) === o3, is(o3));
console.log(is(1), ie(1), pe(5));
const dd: any = Object.getOwnPropertyDescriptor(Object, "seal");
console.log(dd.value === Object.seal, dd.writable);
