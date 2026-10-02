// A class method with a struct-typed parameter called through `any`
// with an array: instance and static dispatch read `x` off the array
// (undefined), the fitting literal takes the typed body, and the
// method read as a value runs with the same reading.
type P = { x: number; y: number };
class K {
  m(p: P) { return p.x; }
  static s(p: P) { return p.x; }
}
const k: any = new K();
console.log("method arr ->", k.m([1, 2]));
console.log("method obj ->", k.m({ x: 4, y: 5 }));
const KK: any = K;
console.log("static arr ->", KK.s([1, 2]));
console.log("static obj ->", KK.s({ x: 4, y: 5 }));
const mv: any = k.m;
console.log("method value arr ->", mv([1, 2]));
console.log("typed", new K().m({ x: 6, y: 7 }), K.s({ x: 8, y: 9 }));
