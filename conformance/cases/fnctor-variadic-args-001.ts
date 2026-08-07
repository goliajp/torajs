// new F(...) with surplus arguments: the factory forwards the full
// construct-site argument list when the body reads `arguments`
// (uniform-argc sites; fn_constructor_argc + static-argv face).
function H() {
  this.n = arguments.length;
}
var h = new H(1, 2, 3);
console.log(h.n);

function P(a: any, b: any) {
  this.top = arguments[2];
  this.left = arguments[3];
  this.a = a;
  this.n = arguments.length;
}
var p = new P(10, 20, 30, 40);
console.log(p.top, p.left, p.a, p.n);

// no-arguments body: surplus args stay dropped (ES semantics), the
// factory keeps its declared-arity shape.
function Q() {
  this.x = 1;
}
var q = new Q(5, 6);
console.log(q.x);

// under-filled uniform site: arguments sees exactly what was passed.
function S(a: any, b: any, c: any) {
  this.n = arguments.length;
  this.b = arguments[1];
}
var s = new S(7, 8);
console.log(s.n, s.b);
