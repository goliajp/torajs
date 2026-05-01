// Adapted from test262: language/expressions/class/* — generic class
// instantiated with concrete type. tr's monomorphization is now
// fixed-point: when the factory `__new_C` is monomorphized, its body's
// inner Calls to `__cm_C__ctor` and `__cm_C__method` (which share the
// class's type_params) are transitively monomorphized too. Each method
// uses the same subst as the outer instantiation.
class Wrapper<T> {
  inner: T;
  constructor(v: T) {
    this.inner = v;
  }
  get_inner(): T {
    return this.inner;
  }
  set_inner(v: T): void {
    this.inner = v;
  }
}

// (Pair<A, B> with mixed types is a known v0 limitation: the factory's
// default-init for TypeVar fields can't pick a representative value
// when the same field's substituted type differs across
// instantiations. Single-type-param classes work; multi-type-param
// classes work iff every TypeVar resolves to the same underlying SSA
// representation. Tracked separately.)
function check(): number {
  // Wrapper<number>.
  let w = new Wrapper(42);
  if (w.get_inner() !== 42) { throw "#1"; }
  w.set_inner(100);
  if (w.get_inner() !== 100) { throw "#2"; }

  // Wrapper<number> — second instance with different value.
  let w2 = new Wrapper(7);
  if (w2.get_inner() !== 7) { throw "#3"; }
  if (w.get_inner() !== 100) { throw "#4: instance independence"; }
  return 0;
}
console.log(check());
