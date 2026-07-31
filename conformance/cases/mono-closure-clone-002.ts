// generic call INSIDE a cloned lifted closure (RFC 20260731 knife 6):
// the clone decls join ast.stmts and closure sites rewrite BEFORE the
// spec body checks, so check_closure walks the clone bodies and their
// inner generic calls get recorded/retargeted (regression: shadowing
// lookup-from-closure — "unknown function `same`" at lower time).
function same<T>(a: T, b: T): void {
  if (a === b) { console.log("ok"); } else { console.log("ne", a, b); }
}
function f5(one) {
  var x = one + 1;
  {
    function f() {
      same(one, 1);
      same(x, 2);
    }
    f();
  }
}
f5(1);
function g(s) {
  const h = () => { same(s, "aa"); };
  h();
}
g("aa");
