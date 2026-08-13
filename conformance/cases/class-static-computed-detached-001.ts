// A symbol-keyed static method read off the class and called detached.
// The S2.38 this-free verdict (a static body has no runtime receiver,
// so a bare call is legal when the argv face is lossless) was computed
// only on the named-static reify path; the computed-key path hardcoded
// it to 0. So this threw "class method called without a receiver" while
// the identically shaped `named` below ran fine -- the same criteria,
// two answers, decided by whether the key was written as an identifier.
class C {
  static [Symbol.hasInstance](n: any): any {
    return n;
  }
  static named(n: any): any {
    return n;
  }
}

const bySymbol = (C as any)[Symbol.hasInstance];
const byName = (C as any).named;

console.log(typeof bySymbol, bySymbol(1));
console.log(typeof byName, byName(2));

// Still reachable the ordinary way, on both keys.
console.log((C as any)[Symbol.hasInstance](4), C.named(5));
