// Nested function declarations inside closure bodies (rotation 254).
// free_vars used to ignore Stmt::FnDecl entirely: the decl's NAME leaked
// into the closure's captures (`__env(inner)`) and the checker rejected
// with "closure references unknown identifier". Now the name hoist-binds
// per statement list and the decl body's own free variables surface.

// 1. non-capturing nested decl
const f = function () {
  function inner() {
    return 42;
  }
  return inner();
};
console.log(f());

// 2. nested decl reading a toplevel binding (test262 `assertions` shape)
let base = 100;
const g = function () {
  function calc() {
    return base + 1;
  }
  return calc();
};
console.log(g());

// 3. hoisting: call site before the declaration
const h = function () {
  const r = pre();
  function pre() {
    return 5;
  }
  return r;
};
console.log(h());

// 4. self-recursive nested decl
const k = function () {
  function fib(n: number): number {
    return n < 2 ? n : fib(n - 1) + fib(n - 2);
  }
  return fib(10);
};
console.log(k());

// 5. nested decl inside a block inside a closure
const m = function () {
  let acc = 0;
  {
    function five() {
      return 5;
    }
    acc = five();
  }
  return acc;
};
console.log(m());

// 6. arrow body with nested decl reading toplevel
const n = () => {
  function read() {
    return base * 2;
  }
  return read();
};
console.log(n());
