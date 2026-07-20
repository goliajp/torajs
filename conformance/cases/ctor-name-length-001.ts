// Builtin ctor own name/length (RFC 20260720-ctor-static-reflection
// 刀 3) — the reified ctor cell answers its ES ctor-clause meta
// through the reflection chain (any-lane reads + gOPD descriptors),
// and the namespace member fold reads the same torajs-rc single
// source (the previous constant length 1 was wrong for Date 7 /
// Symbol, Map, Set 0).
const c1: any = Date;
console.log(c1.name, c1.length);
const c2: any = Symbol;
console.log(c2.name, c2.length);
const c3: any = Map;
console.log(c3.name, c3.length);
console.log(Date.length, Map.length, Set.length, Promise.length, Symbol.length);
const nd: any = Object.getOwnPropertyDescriptor(Date, "name");
console.log(nd.value, nd.writable, nd.enumerable, nd.configurable);
const ld: any = Object.getOwnPropertyDescriptor(Date, "length");
console.log(ld.value, ld.writable);
