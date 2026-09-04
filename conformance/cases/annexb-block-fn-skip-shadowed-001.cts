// Annex B §B.3.3.1 step 1.a — the var binding exists only when
// replacing the declaration with `var F` would raise no early error,
// and only when F is not a parameter name. Both are all-or-nothing:
// when either holds, §B.3.3 is skipped entirely and the outer name
// keeps belonging to whatever already owns it.
//
// bun reads every file as strict code, where §B.3.3 never applies, so
// it cannot tell these apart from the shapes that DO get the binding.
// node agrees with the spec on every line; hence a `.expected`.

// A `let` at the same scope. `f` stays 123 — no binding is created and
// no reference is redirected to the block function.
let f = 123;
if (true) function f() { return "no"; }
console.log("lex same scope", f);

// A `let` in an enclosing block.
try {
  {
    let g = 456;
    if (true) function g() { return "no"; }
    console.log("lex enclosing", g);
  }
} catch (e) {
  console.log("lex enclosing threw");
}
console.log("lex enclosing leaked", typeof g);

// A `for` head binds for the body.
for (let h; ; ) {
  if (true) function h() { return "no"; }
  break;
}
console.log("for head", typeof h);

// So does a for-in element binding.
for (let i in { k: 0 }) {
  if (true) function i() { return "no"; }
}
console.log("for-in head", typeof i);

// A parameter of the same name keeps its argument across the
// declaration (§B.3.3.1 step 1.a.iii).
var init, after;
(function (p) {
  init = p;
  if (true) function p() { return "no"; }
  after = p;
})(123);
console.log("param", init, after);

// And the default-valued form of the same thing.
var dinit, dafter;
(function (q = 456) {
  dinit = q;
  if (true) function q() { return "no"; }
  dafter = q;
})();
console.log("dflt param", dinit, dafter);

// Nothing shadows this one, so it still gets the binding.
console.log("unshadowed pre", typeof r);
if (true) function r() { return "yes"; }
console.log("unshadowed post", typeof r, r());
