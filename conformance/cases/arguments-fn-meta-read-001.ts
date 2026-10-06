// Reading a function's own `length` / `name` never calls it, so it
// does not hide any call site: a function whose value is only called
// directly and read this way still gets its real arguments object.
// `length` counts the declared parameters only, never the slots an
// arguments face adds behind them.
var f_arg = function (x, y) { return arguments; };
console.log(f_arg.length, f_arg.name);
console.log(f_arg(1, 2, 3).length);
console.log(f_arg(1).length, f_arg()[0]);

function g(x, y) { return arguments; }
console.log(g.length, g.name);
const r = g(1, 2, 3);
console.log(r.length, r[2]);

const h = function (a, b, c) { return arguments.length + ":" + arguments[2]; };
console.log(h.length, h(1), h(1, 2, 3, 4));

function k(x) { return arguments[1]; }
console.log(k.length, k(1, "two"));

function m(a, b = 2) { return arguments.length; }
console.log(m.length, m(1, 2, 3, 4, 5));

var p = function (x, y) { return arguments; };
const pa: any = p;
console.log(pa.length, p(1, 2, 3).length);

var F = function (a) { this.n = arguments.length; };
console.log(F.length, F.name, new F(1, 2, 3).n);

// a write to the metadata through any is the strict-mode TypeError
var w = function (x) { return arguments.length; };
try { (w as any).length = 9; } catch (e) { console.log("ro"); }
console.log(w.length, w(1, 2));
