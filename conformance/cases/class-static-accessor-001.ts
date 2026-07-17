// RFC 20260718-accessor-reify 刀 3 — static accessors: a same-name get/set
// pair no longer collides (the faces desugar with _get/_set suffixes),
// C.s reads/writes route through the accessor bodies, and gOPD(C, "s")
// answers a real AccessorPair own entry with reified faces.
class C {
  static _v: number = 5;
  static get s(): number { return C._v; }
  static set s(v: number) { C._v = v; }
  static get ro(): number { return 9; }
  static m(): number { return 1; }
}
console.log("read", C.s);
C.s = 7;
console.log("after-write", C.s, C._v);
console.log("ro", C.ro);
console.log("method", C.m());

const d: any = Object.getOwnPropertyDescriptor(C, "s");
console.log("has-desc", d !== undefined);
console.log("get-type", typeof d.get, "set-type", typeof d.set);
console.log("enum", d.enumerable, "conf", d.configurable);
console.log("get-name", d.get.name, "set-name", d.set.name);
console.log("get-len", d.get.length, "set-len", d.set.length);
console.log("get-call", d.get.call(C));
d.set.call(C, 3);
console.log("after-set-call", C.s);

const dro: any = Object.getOwnPropertyDescriptor(C, "ro");
console.log("ro-get-type", typeof dro.get, "ro-set", dro.set);
console.log("ro-get-call", dro.get.call(C));

// static method reify untouched
const dm: any = Object.getOwnPropertyDescriptor(C, "m");
console.log("m-is-data", typeof dm.value, "m-get", dm.get);
