// §14.3.2 — a destructuring `var` hoists to the enclosing function
// like any other `var`.
//
// The parser desugars a pattern into one binding per name, and the
// `var`-ness of the declaration was not travelling with them: every
// name came out block-scoped, so reading one after its block answered
// `unknown identifier`. `let` and `const` patterns are unaffected —
// they really are block-scoped, which the second half checks.

const src: any = { a: 1, b: 2, rest1: 3, rest2: 4 };
const arr: any = [10, 20, 30];

function f(): any {
  {
    var { a, b } = src;
    var [x, y] = arr;
  }
  // Out of the block, still in the function.
  return [a, b, x, y];
}
console.log(f());

// Rest bindings take the same route.
function g(): any {
  {
    var { a, ...others } = src;
    var [head, ...tail] = arr;
  }
  return [a, others.rest1, head, tail.length];
}
console.log(g());

// Nested patterns recurse, so the inner names hoist too.
function h(): any {
  const nested: any = { outer: { inner: 7 } };
  {
    var {
      outer: { inner },
    } = nested;
  }
  return inner;
}
console.log(h());

// The control: a `let` pattern is block-scoped, and reading it after
// the block is a different program entirely — so it is read inside.
function k(): any {
  let out: any = 0;
  {
    let { a } = src;
    out = a;
  }
  return out;
}
console.log(k());
