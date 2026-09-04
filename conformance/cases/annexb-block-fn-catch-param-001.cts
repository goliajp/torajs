// Annex B §B.3.3 — a catch parameter does not stand between the block
// function's write and the binding it is addressed to.
//
// §B.3.3.1 step 3.a.ii.2 writes the value with
// `varEnv.SetMutableBinding(F, ...)`, and `varEnv` is the enclosing
// function's (or the script's) VariableEnvironment. A catch parameter
// lives in its own declarative environment wrapped around the catch
// block, so it is not `varEnv` — a `catch (f)` around a block that
// declares `function f` therefore does NOT intercept the write. After
// the try, the function is visible outside; inside the catch block,
// `f` is still the caught value.
//
// §B.3.5 is the other half: it permits the collision in the first
// place, as long as the catch parameter is a plain BindingIdentifier.
//
// bun is not an oracle here: it answers `undefined` for the post-try
// read of every case below. node agrees with the spec, so this checks
// against a `.expected`.

// The test262 `no-skip-try` shape, in a function body.
(function () {
  console.log("fn pre", typeof f);
  try {
    throw null;
  } catch (f) {
    console.log("fn in-catch", f);
    if (true) function f() { return 123; }
    console.log("fn in-catch after", typeof f);
  }
  console.log("fn post", typeof f, f());
})();

// The same at script top level, where `varEnv` is the script's.
try {
  throw null;
} catch (g) {
  { function g() { return "g"; } }
}
console.log("top post", typeof g, g());

// The catch parameter still binds normally: it is readable, writable,
// and its value does not escape the block.
var h = "outer";
try {
  throw "caught";
} catch (h) {
  console.log("param read", h);
  h = "assigned";
  console.log("param write", h);
  if (true) function h() { return "fn"; }
  console.log("param after decl", h);
}
console.log("outer h", typeof h, h());

// A catch parameter of a different name is untouched by any of this.
try {
  throw null;
} catch (e) {
  if (true) function k() { return "k"; }
  console.log("other-name in-catch", e, typeof k);
}
console.log("other-name post", typeof k, k());
