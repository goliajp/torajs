// §13.3.7 — a class whose builtin heritage was stripped still has a
// super base, and it is the BUILTIN prototype (the class object for a
// static member), not %Object.prototype%. The name forms take their
// own runtime re-dispatch; the computed forms read the base, so they
// are the ones that saw the wrong object.
class MySet extends Set<number> {
  h1(x: number) { return super["has"](x); }
  h2(x: number) { return super.has(x); }
  addTwo(a: number, b: number) { super["add"](a); super.add(b); return this.size; }
  // A static member's home object is the class, so its super base is
  // the parent CLASS object.
  static parentName() { return super["name"]; }
}
const s = new MySet();
console.log(s.addTwo(1, 2), s.h1(2), s.h1(9), s.h2(1));
console.log(MySet.parentName());

class MyArr extends Array<number> {
  j() { return super["join"]("-"); }
  n() { return super["length"]; }
}
const a = new MyArr();
a.push(1);
a.push(2);
console.log(a.j(), typeof a.n());

class MyMap extends Map<string, number> {
  g(k: string) { return super["get"](k); }
  // §13.3.7 keeps the CURRENT `this` as the receiver, so the builtin
  // reads this instance's storage rather than the prototype's.
  size2() { return super["get"]("k") === 5; }
}
const m = new MyMap();
m.set("k", 5);
console.log(m.g("k"), m.size2());
