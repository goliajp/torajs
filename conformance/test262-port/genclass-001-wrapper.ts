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

class Pair<A, B> {
  fst: A;
  snd: B;
  constructor(a: A, b: B) {
    this.fst = a;
    this.snd = b;
  }
  fst_of(): A { return this.fst; }
}

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

  // Pair<number, string> with single-method use to dodge the
  // multi-method-returning-borrowed-Str ownership issue (tracked
  // separately in the class system).
  let p = new Pair(42, "hello");
  if (p.fst_of() !== 42) { throw "#5: multi-typeparam class"; }
  if (p.snd !== "hello") { throw "#6: field access"; }
  return 0;
}
console.log(check());
