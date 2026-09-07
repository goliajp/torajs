// Annex B §B.3.4 — `if (c) FunctionDeclaration` has the semantics of
// `if (c) { FunctionDeclaration }`, so the declaration is block-scoped
// and §B.3.3 owes it a var-scoped binding besides. The two slots are
// independent: the body assigns the BLOCK one, the var one keeps the
// function. bun answers `f is not defined` here (it gives the bare
// branch no var binding at all), so the assertions are spelled out
// against the spec rather than against bun.
var initialBV: any, currentBV: any, varBinding: any;

(function () {
  if (true) function f() { initialBV = f; f = 123; currentBV = f; return "decl"; }
  varBinding = f;
  f();
})();

console.log(initialBV());
console.log(currentBV);
console.log(varBinding());

// else-clause spelling of the same production
var elseBV: any, elseVar: any;
(function () {
  if (false) ; else function g() { elseBV = g; g = 7; return "else"; }
  elseVar = g;
  g();
})();
console.log(elseBV());
console.log(elseVar());

// both branches are declarations
var bothVar: any;
(function () {
  if (true) function h() { return "then"; } else function h() { return "else"; }
  bothVar = h;
})();
console.log(bothVar());

// the braced spelling must keep answering exactly the same
var bracedBV: any, bracedVar: any;
(function () {
  if (true) { function k() { bracedBV = k; k = 5; return "braced"; } }
  bracedVar = k;
  k();
})();
console.log(bracedBV());
console.log(bracedVar());
