// Function values of another face than the fn-typed slot they reach,
// in three shapes: a callee cloned for the value that calls itself, a
// mutable fn-typed binding captured by closures (it becomes a module
// global), and arguments the value does not declare whose evaluation
// has side effects.
function mk2(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

// the clone's recursive call reaches the clone again
function rec(h: (y?: number) => number, n: number): number {
  if (n === 0) return h();
  return h(n) + rec(h, n - 1);
}
const f = (y?: number) => (y === undefined ? 5 : y) + 1;
function exact(y?: number): number { return y === undefined ? 100 : y }
console.log(rec(f, 3), rec(mk2(10), 2), rec(exact, 2));

// mutual recursion through two cloned callees
function ping(h: (y?: number) => number, n: number): number { return n <= 0 ? h() : pong(h, n - 1) + 1 }
function pong(h: (y?: number) => number, n: number): number { return n <= 0 ? h(7) : ping(h, n - 1) * 2 }
console.log(ping(mk2(1), 3), ping(exact, 4));

// a mutable binding captured by closures
let m: (y?: number) => number = mk2(1);
const viaArrow = () => m(10);
const twice = (y?: number) => m(y) + m();
console.log(viaArrow(), twice(), twice(1));
m = mk2(2);
console.log(viaArrow(), twice(), twice(1));
m = exact;
console.log(viaArrow(), twice(), twice(1));

// arguments the value does not declare are still evaluated, in order
const log: string[] = [];
function note(s: string, v: number): number { log.push(s); return v }
function three(h: (a: number, b: number, c: number) => number) {
  return h(note("a", 1), note("b", 2), note("c", 3));
}
function mkOne(k: number) { return (a?: number) => (a === undefined ? -1 : a) + k }
console.log(three(mkOne(10)), log.join(","));
log.length = 0;
const one1: (a: number, b: number) => number = mkOne(20);
console.log(one1(note("x", 4), note("y", 5)), log.join(","));
