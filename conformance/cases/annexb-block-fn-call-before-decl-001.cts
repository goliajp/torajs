// A block function is initialized on block entry (§14.2.3), so a call
// written above its text works; Annex B §B.3.3 adds a var binding that
// is written only where the text sits. tr pointed the block's own
// references at the var binding too, so the early call read `undefined`
// and threw `TypeError`.
//
// bun is not an oracle here: it reads every file as strict code, where
// there is no var binding and `typeof f` after the block is
// "undefined". The `.expected` holds the spec's answers, which are
// node's except where noted.

function k() {
  {
    console.log(f());
    function f() { return 1; }
  }
  console.log(typeof f);
}
k();

{
  console.log(t());
  function t() { return 2; }
}
console.log(typeof t);

// the var binding is still unwritten while the block runs up to the text
function probe() {
  const outside = () => typeof g;
  {
    console.log(outside(), g());
    function g() { return 3; }
    console.log(outside());
  }
}
probe();

// a nested block that declares the name again owns it there, and
// writes no var: the outer block declares `h` lexically, so replacing
// the inner declaration with `var h` would be an early error and
// §B.3.3.1 step 1.a.ii leaves it alone. The last call reads the var
// the outer declaration wrote (test262
// annexB/language/function-code/block-decl-nested-blocks-with-fun-decl.js
// asserts the same; node answers "inner" here, against the spec).
function twice() {
  {
    console.log(h());
    function h() { return "outer"; }
    {
      console.log(h());
      function h() { return "inner"; }
    }
  }
  console.log(h());
}
twice();

// a function expression body is its own var scope
const viaExpr = function () {
  {
    console.log(e());
    function e() { return 4; }
  }
  return typeof e;
};
console.log(viaExpr());
