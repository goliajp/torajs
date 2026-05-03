// Phase M-OO.4 — static fields + static methods.
//
// `static fieldName: T = init` desugars to a top-level
//   `let __sf_<Class>__<name>: T = init;`
// (const-form, picked up by K.3/K.4 globals machinery).
//
// `static methodName(args): R { body }` desugars to a top-level
//   `function __sm_<Class>__<name>(args): R { body }`
// (no `__this` param — statics don't bind a receiver).
//
// `<Class>.<member>` access at the source level is rewritten by
// `desugar_classes` to `__sf_<Class>__<member>` / `__sm_<Class>__<member>`
// Ident, so check.rs and ssa_lower see plain top-level references.

class Counter {
  static initial: number = 100;
  static label: string = "ctr";

  value: number;

  constructor(start: number) {
    this.value = start;
  }

  static fresh(): Counter {
    return new Counter(Counter.initial);
  }

  static doubled(n: number): number {
    return n * 2;
  }

  bump(): number {
    this.value = this.value + 1;
    return this.value;
  }
}

function check(): number {
  if (Counter.initial !== 100) { throw "#1: static field read"; }
  if (Counter.label !== "ctr") { throw "#2: static string field"; }
  if (Counter.doubled(7) !== 14) { throw "#3: static method"; }

  let c: Counter = Counter.fresh();
  if (c.value !== 100) { throw "#4: static method returning instance"; }
  if (c.bump() !== 101) { throw "#5: instance method on static-built instance"; }
  if (c.bump() !== 102) { throw "#6: instance method again"; }

  // Static method called from another fn body, not just top-level.
  if (Counter.doubled(Counter.doubled(3)) !== 12) { throw "#7: nested static call"; }

  return 0;
}
console.log(check());
