// A nested `function` declaration whose enclosing scope is a function
// EXPRESSION body. `desugar_nested_fns` had a pass for a top-level
// FnDecl body and one for module-top-level statements; a function
// expression's body was neither, so nothing collected the declarations
// inside it until `lift_arrow_fns` had turned that body into a
// top-level declaration — by which point the body's other function
// values had been lifted out too, and the rename of the declaration
// could no longer reach the references inside them.

(function () {
  function F(): number {
    return 7;
  }
  const t = function (): number {
    return F();
  };
  console.log(t());
})();

// Same body, an arrow doing the reading.
(function () {
  function F(): number {
    return 7;
  }
  const t = (): number => F();
  console.log(t());
})();

// Not only an IIFE: any function-expression body is the missing scope.
function outer(): void {
  const a = function (): number {
    function F(): number {
      return 7;
    }
    const b = (): number => F();
    return b();
  };
  console.log(a());
}
outer();

// A self-recursive declaration is in this lane too — recursion into a
// peer is not a capture, so `desugar_capturing_nested_fns` leaves it
// here rather than routing it to the closure lane.
(function () {
  function fact(n: number): number {
    return n <= 1 ? 1 : n * fact(n - 1);
  }
  const t = (): number => fact(5);
  console.log(t());
})();

// The neighbouring shapes that already worked must keep working: a
// declaration read directly in the same body, and one whose scope is a
// top-level declaration's body.
(function () {
  function F(): number {
    return 7;
  }
  console.log(F());
})();

function host(): void {
  function F(): number {
    return 7;
  }
  const t = (): number => F();
  console.log(t());
}
host();

// A declaration that captures never reaches this pass — it is rewritten
// into a closure earlier — and must still answer.
(function () {
  const o = 3;
  function g(): number {
    return o;
  }
  const t = (): number => g();
  console.log(t());
})();
