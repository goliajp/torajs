// A nested function declaration reading `this` owns its receiver: a
// plain call answers undefined (strict module goal), never the
// enclosing method's instance - and the lifted closure's return
// annotation must not pin the enclosing receiver's class on it
// (storing the returned undefined through a class-typed slot was a
// store/drop SIGSEGV).
class C {
  v = 5;
  m() {
    function inner() {
      return this;
    }
    const stored = inner();
    return [this.v, stored, inner()];
  }
}
console.log(new C().m());

class D {
  m() {
    function inner() {
      return this;
    }
    return [1, inner()];
  }
}
console.log(new D().m());

function outer(this: any) {
  const self = this;
  function inner() {
    return this;
  }
  return [self, inner()];
}
console.log(outer.call({ x: 1 }));
