// `Expr::New` carries its target as a NAME, and `desugar_classes`
// binds every one of them to the `__new_<C>` factory of the class with
// that name — a global lookup with no notion of scope. A `function N`
// declared in a function body shadows a class `N` for the `new`s
// written inside it, but the rename that establishes that shadow is
// `desugar_nested_fns`, twenty passes later; by then the `new` is a
// call to a factory and the rename has nothing to bind to.
//
// Silently, because both answer to the same name: the program got the
// class's instance, fields and all.

class Widget {
  tag: number = 1;
}

(function () {
  function Widget(): void {}
  const x: any = new (Widget as any)();
  console.log(x.constructor === Widget, x.tag);
})();

// The shadowing scope does not have to be a function expression.
function host(): void {
  function Widget(): void {}
  const x: any = new (Widget as any)();
  console.log(x.constructor === Widget, x.tag);
}
host();

// The builtin error hierarchy is injected as ordinary classes, so a
// program shadowing one of those names inside a function body hit the
// same lookup — and there the wrong answer is an intrinsic.
(function () {
  function TypeError(): void {}
  const x: any = new (TypeError as any)();
  console.log(x.constructor === TypeError);
})();

(function () {
  function RangeError(): void {}
  const x: any = new (RangeError as any)();
  console.log(x.constructor === RangeError);
})();

// Read through an inner function value: the shadow holds there too.
(function () {
  function TypeError(): void {}
  const make = function (): any {
    return new (TypeError as any)();
  };
  console.log(make().constructor === TypeError);
})();

// Controls. An unshadowed `new` still reaches the class, a top-level
// shadow was already answered (604-05), and a `new` through a binding
// was never on the name lookup at all.
const plain: any = new Widget();
console.log(plain.constructor === Widget, plain.tag);

(function () {
  const C: any = Widget;
  const y: any = new C();
  console.log(y.constructor === Widget, y.tag);
})();
