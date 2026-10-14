// A function declaration on the left of instanceof is the function object:
// the operator throws a TypeError for a target that is not callable and has
// no @@hasInstance (ECMA-262 InstanceofOperator), and a handler on the target
// receives the function.

function attempt(label: string, f: () => void) {
  try {
    f();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, e instanceof TypeError);
  }
}
attempt("nested", () => {
  function foo() {}
  let obj = {};
  console.log(foo instanceof obj);
});
function bar() {}
const plain = {};
attempt("top", () => {
  console.log(bar instanceof plain);
});
attempt("handler", () => {
  function baz() {}
  let o = {
    [Symbol.hasInstance](v: any) {
      return typeof v === "function" && v === baz;
    },
  };
  console.log(baz instanceof o);
});
attempt("builtin", () => {
  function q() {}
  console.log(q instanceof Function, q instanceof Object);
});
