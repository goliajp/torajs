// A function a generator declares, lifted out of its body, is still
// what `new` reaches in the body, in its closures and in its classes.

function* direct() {
  function Foo(this: any) {
    this.v = 1;
  }
  const a = new (Foo as any)();
  yield a.constructor === Foo;
  yield a.v;
}

function* closure() {
  function Bar(this: any) {
    this.v = 2;
  }
  const f = () => new (Bar as any)();
  yield f().constructor === Bar;
  yield f().v;
}

function* method() {
  function Baz(this: any) {
    this.v = 3;
  }
  class K {
    m() {
      return new (Baz as any)();
    }
  }
  const b = new K().m();
  yield b.constructor === Baz;
  yield b.v;
}

function* builtinName() {
  function RangeError(this: any) {
    this.v = 4;
  }
  class K {
    m() {
      return new (RangeError as any)();
    }
  }
  yield new K().m().constructor === RangeError;
}

console.log([...direct()]);
console.log([...closure()]);
console.log([...method()]);
console.log([...builtinName()]);
