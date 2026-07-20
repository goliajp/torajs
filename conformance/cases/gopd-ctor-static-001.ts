// Object.getOwnPropertyDescriptor over builtin ctor statics (RFC
// 20260720-ctor-static-reflection 刀 2) — a ctor cell answers its
// ns-static table entries as own data descriptors { writable: true,
// enumerable: false, configurable: true } whose value is the SAME
// interned cell a value read mints (identity holds), callable and
// name/length-reflective; a non-table key stays undefined.
const d: any = Object.getOwnPropertyDescriptor(Date, "parse");
console.log(typeof d.value, d.writable, d.enumerable, d.configurable);
console.log(d.value === Date.parse);
console.log(Object.getOwnPropertyDescriptor(Date, "nosuch"));
const d2: any = Object.getOwnPropertyDescriptor(Object, "keys");
console.log(typeof d2.value, d2.value === Object.keys);
const d3: any = Object.getOwnPropertyDescriptor(String, "fromCharCode");
console.log(d3.value(72, 105));
const dn: any = Object.getOwnPropertyDescriptor(Date, "now");
console.log(dn.value.name, dn.value.length);
