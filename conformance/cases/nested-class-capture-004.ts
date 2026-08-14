// The captured binding is written AFTER the class is declared and
// after an instance exists: the method reads the binding, not a copy
// taken at class-evaluation time.
function outer(): number {
  let a = 1;
  class K {
    m(): number {
      return a;
    }
  }
  const k = new K();
  a = 5;
  return k.m();
}
console.log(outer());

// instanceof answers off the prototype link.
function tag(): boolean {
  const b = 2;
  class K {
    m(): number {
      return b;
    }
  }
  return new K() instanceof K;
}
console.log(tag());
