// A block function's block binding is a variable: the block may assign
// it, and every entry into the block makes a fresh one holding the
// function again (§14.2.3 BlockDeclarationInstantiation). tr pointed
// the block's references at the one lifted function, so `f = null`
// wrote a program-wide name — the next pass through the loop found
// `null`, or the checker refused the write. The Annex B var binding
// gets the block binding's value at the declaration's text (§B.3.3.1).
//
// bun is not an oracle here (it reads every file as strict code, with
// no var binding). The `.expected` holds the spec's answers, which are
// node's.

for (let i = 0; i < 3; i++) {
  console.log(f());
  function f() { return 1; }
  f = null;
}
console.log(typeof f);

function k() {
  for (let i = 0; i < 2; i++) {
    console.log(typeof g, g());
    function g() { return 2; }
    g = 5;
    console.log(g);
  }
  console.log(typeof g);
}
k();

// a write from a closure in the block lands on the block binding too
function viaArrow() {
  for (let i = 0; i < 2; i++) {
    console.log(typeof h);
    const clear = () => { h = undefined; };
    function h() {}
    clear();
    console.log(typeof h);
  }
  console.log(typeof h);
}
viaArrow();

// an enclosing `let` or a parameter owns the name, so there is no var
// binding (§B.3.3.1 step 1.a.ii), but the block binding still holds the
// function from the block's first statement on
function ownedByLet() {
  let f = 1;
  {
    console.log(f());
    function f() { return 2; }
  }
  console.log(f);
}
ownedByLet();

function ownedByParam(f) {
  {
    console.log(f());
    function f() { return 3; }
  }
  console.log(f);
}
ownedByParam(4);
