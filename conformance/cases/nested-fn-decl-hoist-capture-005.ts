// Function declarations in a generator body. A group that reads nothing
// around it stays a declaration (a direct call) and must still be in
// reach after a `yield`; one that reads the body's locals becomes a
// value capturing the machine's frame; a generic or `arguments`-reading
// member keeps its group declarations.

function* plain() {
  function f(): number { return 1; }
  function g<T>(x: T): number { return f(); }
  yield g(0);
  yield f();
}
for (const v of plain()) console.log(v);

function* args() {
  function f(): number { return 2; }
  function g(): number { return arguments.length + f(); }
  yield g();
}
for (const v of args()) console.log(v);

function* mixed(n: number) {
  let total = 0;
  function add(m: number): number { total += m; return total; }
  function twice(m: number): number { return add(m) + add(m); }
  function pure(m: number): number { return m * 3; }
  yield twice(n);
  yield pure(n);
  yield total;
}
for (const v of mixed(2)) console.log(v);

function* afterYield(n: number) {
  function f(): number { return 1; }
  function g(x: number): number { return f() + x; }
  yield 0;
  yield f();
  yield g(n);
}
for (const v of afterYield(5)) console.log(v);

const genExpr = function* (n: number) {
  yield 0;
  yield sq(n);
  function sq(m: number): number { return m * m; }
};
for (const v of genExpr(4)) console.log(v);
