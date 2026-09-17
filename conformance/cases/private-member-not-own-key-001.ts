// §7.3 — a private element is not a property: no reflection face
// reports it. tr reifies a private method / accessor as an own entry
// of the prototype (that entry carries the brand), so the walks skip
// the mangled row instead of the entry not existing.
class C {
  #m() { return 1 }
  get #g() { return 2 }
  pub() { return this.#m() + this.#g }
}
console.log(Object.getOwnPropertyNames(C.prototype));
console.log(Reflect.ownKeys(C.prototype));
const c = new C();
console.log(c.pub());
console.log(c);
const seen: string[] = [];
for (const k in C.prototype) { seen.push(k) }
console.log(seen);
console.log(Object.keys(Object.getOwnPropertyDescriptors(C.prototype)));

class S {
  static #x = 1;
  static bump() { S.#x += 1; return S.#x }
}
console.log(Object.keys(S));
console.log(Object.getOwnPropertyNames(S).filter((k) => k !== "prototype" && k !== "length" && k !== "name"));
console.log(S.bump());
console.log({ ...S });
