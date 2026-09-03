// Legal shapes the single-statement-position declaration check
// (§14.6 / §14.7 / §14.13, parser reject_decl_in_single_stmt) must
// NOT reject. The illegal matrix (let/const/class/generator/async fn
// as an if/else/loop/label body) is REJ-BOTH-verified against bun.
// Note the declarations themselves must parse but their bindings do
// not escape to this scope under bun — declare only, never call.

// The Annex B.3.2/B.3.4 shapes (`if (1) function f() {}` and
// `lbl: function g() {}`) moved to `annexb-fn-decl-sloppy-001.cts`
// in rotation 578 — both productions are sloppy-only, and this file
// is a module.
console.log("decls parsed");

// block bodies are Statement proper — decls inside are fine
for (let i = 0; i < 1; i++) console.log(i);
if (1) {
  let k = 5;
  console.log(k);
}
lbl2: {
  console.log("in");
  break lbl2;
}
