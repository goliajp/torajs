// Capturing nested function declarations inside closure bodies
// (rotation 254 blade 2). The capturing-nested-fn router only walked
// the statement tree, so a declaration nested in a function-expression
// or arrow body (expression arena) never routed — nested_fns lifted it
// to top level where the closure's params and locals resolve to
// nothing. The router now scans the arena; the minted closure lowers
// after its parent via the Pass 2B topological order.

// 1. nested decl reading the closure's param
Promise.resolve(7).then(function (v) {
  function helper() {
    return v + 1;
  }
  console.log(helper());
});

// 2. nested decl reading a closure local
const f = function () {
  let acc = 10;
  function bump() {
    return acc + 5;
  }
  return bump();
};
console.log(f());

// 3. mutually recursive pair (no outer capture — stays on the lift lane)
const g = function (n: number) {
  function even(k: number): boolean {
    return k == 0 ? true : odd(k - 1);
  }
  function odd(k: number): boolean {
    return k == 0 ? false : even(k - 1);
  }
  return even(n);
};
console.log(g(10));

// 4. arrow body, nested decl reading the arrow's param
const h = (x: number) => {
  function twice() {
    return x * 2;
  }
  return twice();
};
console.log(h(21));

// 5. two levels: closure in closure, inner decl reads both params
const outer = function (a: number) {
  const inner = function (b: number) {
    function sum() {
      return a + b;
    }
    return sum();
  };
  return inner(2);
};
console.log(outer(40));
