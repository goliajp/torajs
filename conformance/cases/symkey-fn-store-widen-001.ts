// r295 — Symbol-keyed fn-value stores + the container-poison scalar
// split (L3b r293 #3). Three faces:
//   1. named-fn / generator-factory / generator-fn-expr RHS on a
//      symbol-keyed index-assign wraps into a forward closure cell
//      (the chunk-733 arm widened to every index-assign target);
//   2. the stored cell reads back callable with `typeof` intact;
//   3. an untrackable index-assign receiver poisons container widths
//      — the poison must reach SCALAR dependents too (`const y = c.x`
//      lowered F64/I64-split before, bit-punning the state load —
//      the materialize_operand_gpr SIGABRT family).
function* gen() { yield 5; yield 6; }
const holder: any = {};
holder[Symbol.iterator] = gen;
const g = holder[Symbol.iterator]();
console.log(g.next().value);
console.log(g.next().value);
console.log(g.next().done);

const h2: any = {};
h2[Symbol.iterator] = function* () { yield 9; };
const g2 = h2[Symbol.iterator]();
console.log(g2.next().value);
console.log(g2.next().done);

class C { x: number = 7; }
const c = new C();
(Array.prototype as any)[Symbol.iterator] = gen;
const y: number = c.x;
console.log(y + 1);

function addOne(n: number): number { return n + 1; }
const fnbox: any = {};
fnbox[Symbol.iterator] = addOne;
const back = fnbox[Symbol.iterator];
console.log(back(41));
console.log(typeof back);
