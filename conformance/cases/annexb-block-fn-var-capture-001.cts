// Annex B §B.3.3 — the var binding a block-nested `function` gets is a
// LOCAL of the holding body, so the closure lift must not capture an
// outer binding of the same name.
//
// The free-variable walk pre-binds `var` names on entry because `var`
// is function-scoped however deep in blocks it is written. §B.3.3
// makes a block-nested `function` function-scoped in sloppy code for
// exactly the same reason, and it was not on that list. So a
// function-expression body that mentioned the name captured the outer
// binding, and the Annex B pass then declared the var binding into the
// very scope the captured env parameter already bound — tr answered
// "redeclaration of `f` in current scope".
//
// The read-only shape hid it for a long time: with nothing writing the
// name there was no outer binding to capture in the first place.
//
// bun is not an oracle here — it answers `undefined` for the post-block
// read of a block function. node follows the spec, so this checks
// against a `.expected`.

// The shape test262's annexB/language/eval-code `func-*-eval-func-init`
// family is written in, with the eval taken out: the write is what
// creates the outer binding, and the binding is what got captured.
var init, changed;
(function () {
  init = f;
  f = 123;
  changed = f;
  {
    function f() {}
  }
  console.log(init, changed, typeof f);
})();
console.log(init, changed, typeof f);

// An outer binding declared by hand, rather than by the implicit-global
// synthesis: same collision, same answer. The outer one is untouched.
var g = "outer";
(function () {
  g = 123;
  {
    function g() {}
  }
  console.log(typeof g);
})();
console.log(g);

// An arrow body reaches it through the same lift.
var h;
var arrow = () => {
  h = 1;
  {
    function h() {}
  }
  console.log(typeof h);
};
arrow();
console.log(h);

// A declaration wrapper never had the bug (it is not lifted), and must
// keep answering the same thing.
var k;
function wrapper() {
  k = 1;
  {
    function k() {}
  }
  console.log(typeof k);
}
wrapper();
console.log(k);

// Still no leak: the var binding stays inside the body that holds it.
(function () {
  {
    function inner() {}
  }
})();
try {
  inner;
} catch (e) {
  console.log("outer read:", e.constructor.name);
}
