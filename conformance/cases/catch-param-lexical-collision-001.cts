// §13.15.1 — "It is a Syntax Error if any element of the BoundNames of
// CatchParameter also occurs in the LexicallyDeclaredNames of Block."
// Annex B B.3.4 rewrites that subclause, but it replaces only the
// VarDeclaredNames clause and only for a plain BindingIdentifier: that
// is what makes `catch (e) { var e }` legal. The lexical clause is
// never waived, so `function e() {}`, `let e` and `class e {}` written
// directly in the block are SyntaxErrors — a function declaration in a
// Block is a LexicallyDeclaredName, in sloppy code as much as strict.
//
// The refusals cannot run. What can is the line the two clauses fall
// on, which is where a check like this goes wrong: too eager and
// `catch (e) { var e }` stops working; too shallow and a nested
// block's own declaration is mistaken for the catch block's.

var log: string[] = [];

// B.3.4's whole point. The parameter is named `v` rather than `e`
// because a `catch (e) { var e }` clause and a `catch (e) { { function
// e(){} } }` clause in the same scope hit an unrelated, pre-existing
// limit further down ("ssa-lower: assign to unknown ident `e`"), and
// this fixture is about the parse.
try {
  throw "a";
} catch (v) {
  var v;
  log.push("var-v " + v);
}

// A nested block's declarations are its own, not the catch block's.
try {
  throw "b";
} catch (e) {
  {
    function e() {
      return "shadow";
    }
  }
  log.push("nested-block " + typeof e);
}

// A different name is nobody's business.
try {
  throw "c";
} catch (e) {
  function other() {
    return "other";
  }
  let alsoOther = "let";
  log.push("other-names " + other() + " " + alsoOther);
}

// And a parameterless catch has no BoundNames at all.
try {
  throw "d";
} catch {
  let e = "no-param";
  log.push(e);
}

console.log(log.join(" | "));
