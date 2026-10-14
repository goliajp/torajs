// A generic class method has two all-`any` bodies in the mono program
// besides its specializations: the twin instance the boxed adapter
// enters for a foreign receiver, and the mono instance its dispatch
// rows enter for a call through a receiver no class type names. Both
// write `this.v = x` with `x` of any kind, so the shadow census counts
// them only when some call can enter them: here a dynamic call with
// an argument the specialization refuses leaves `v` holding a string
// in a cell laid out for a number, and the typed reads answer it.
class L<T> {
  v!: T;
  set(x: T) { this.v = x }
}
const a = new L<number>();
a.set(1);
console.log(a.v + 1);
const d: any = a;
d.set("s");
console.log(a.v, typeof a.v);

class M<T> {
  w!: T;
  put(x: T) { this.w = x }
}
const m = new M<string>();
m.put("q");
console.log(m.w, m.w.length);
