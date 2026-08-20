// generator body destructuring of non-Array iterables — the lifted
// group temp must step the iterator protocol, not index the source.
const log: string[] = [];
const iterator = {
  next() { log.push("next"); return { done: false, value: log.length }; },
  return() { log.push("return"); return {}; }
};
const iterable: any = {};
iterable[Symbol.iterator] = function() { return iterator; };

// g1 — custom iterable behind any, bounded pattern (2 steps + close)
function* g1() {
  const [a, b] = iterable;
  yield a;
  yield b;
}
const i1 = g1();
console.log("g1", i1.next().value, i1.next().value, JSON.stringify(log));

// g2 — Set source, lifted across a later yield
function* g2() {
  const s = new Set([10, 20, 30]);
  const [x, y] = s;
  yield x;
  yield y;
}
const i2 = g2();
console.log("g2", i2.next().value, i2.next().value);

// g4 — dstr ASSIGNMENT form (existing bindings) in a generator
const iterator4 = {
  next() { log4.push("next"); return { done: false, value: log4.length }; },
  return() { log4.push("return"); return {}; }
};
const iterable4: any = {};
iterable4[Symbol.iterator] = function() { return iterator4; };
function* g4() {
  let m: any, n: any;
  [m, n] = iterable4;
  yield m;
  yield n;
}
const log4: string[] = [];
const i4 = g4();
console.log("g4", i4.next().value, i4.next().value, JSON.stringify(log4));

// g5 — array-literal source keeps its typed lane (no protocol walk)
function* g5() {
  const [u, v] = [100, 200];
  yield u + v;
}
console.log("g5", g5().next().value);
