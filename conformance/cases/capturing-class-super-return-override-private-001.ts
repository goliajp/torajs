// the adopted object carries the derived class's private field; a
// non-extensible one refuses it (bun does not implement that refusal,
// hence the .expected).
// implicit ctor + private field lands on the returned object
class M extends (function (this: any, x: any) { return x; } as any) {
  #mark = "bar";
  static has(o: any): boolean { return #mark in o; }
  static read(o: any): string { return o.#mark; }
}
const t: any = {};
const r: any = new M(t);
console.log(r === t, M.has(t), M.read(t), M.has({}));
const sealed: any = Object.preventExtensions({});
try { new M(sealed); console.log("no throw"); } catch (e) { console.log(e instanceof TypeError); }
console.log(M.has(sealed));
