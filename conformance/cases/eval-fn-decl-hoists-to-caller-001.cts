// §19.2.1.1 — a SLOPPY direct eval's function declarations belong to
// the CALLER's VariableEnvironment and are instantiated when the eval
// is evaluated, not where they sit in the text. Reading the name from a
// statement written BEFORE the declaration, inside the same eval, finds
// the function.
//
// tr inlines an eval as a block — that is the eval's LexicalEnvironment
// and it is why `let` stays inside — so the declarations have to come
// out of it, or Annex B §B.3.3 reads them as block-nested and gives
// each a binding written at its own position instead.
//
// bun reads every file as strict code, where a sloppy eval's scope
// rules do not apply, so the oracle here is a `.expected` (node agrees
// with the spec on every line).

var initial;
eval('initial = f; function f() { return "first"; } function f() { return "second"; }');
console.log("multi", typeof initial, initial(), f());

var local;
(function () {
  var g = 88;
  eval('local = g; function g() { return 33; }');
})();
console.log("local", typeof local, local());

// The eval's own top-level declaration is hoisted; one nested in a
// block inside it still gets the Annex B write, which lands on the same
// binding — so the read before the block sees the hoisted one.
var both;
(function () {
  eval('both = h; { function h() { return 1; } } function h() { return 2; }');
})();
console.log("both", both());

// `let` does NOT come out: the eval's lexical scope stays its own.
eval('let contained = 5;');
console.log("lexical", typeof contained);
