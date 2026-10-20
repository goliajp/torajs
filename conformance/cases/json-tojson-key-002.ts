// an any-typed member of a composite the compiler unfolds: its toJSON
// hook receives the key the member sits under (§25.5.2.3 step 2), the
// field name in a struct or class instance and the index in an array
function hook(this: any, key: any) {
  return arguments.length + ":" + typeof key + ":" + key;
}
const h: any = { toJSON: hook };
const s = { a: h, b: 1 };
console.log(JSON.stringify(s));
type P = { x: any; y: number };
const p: P = { x: h, y: 2 };
console.log(JSON.stringify(p));
class C {
  f: any;
  constructor(v: any) {
    this.f = v;
  }
}
console.log(JSON.stringify(new C(h)));
console.log(JSON.stringify([s]));
console.log(JSON.stringify({ outer: s }));
const xs: any[] = [h, 1, h];
console.log(JSON.stringify(xs));
console.log(JSON.stringify(s, null, 2));
console.log(JSON.stringify({ q: [h] }));
const m = { k: { toJSON: hook } };
console.log(JSON.stringify(m));
console.log(JSON.stringify({ deep: { list: [new C(h)] } }, null, 1));
