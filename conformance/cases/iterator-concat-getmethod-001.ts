// proposal-iterator-sequencing §27.1.2.1 step 2 runs
// GetMethod(item, @@iterator) for every item EAGERLY and stores the
// answer as [[OpenMethod]]; step 3 opens each item by CALLING the
// stored method. tr checked presence eagerly and re-walked the symbol
// at open time. A data property cannot tell those apart; an accessor
// can, three ways: the getter ran at the first next() instead of at
// concat(), a second walk would have run it twice, and a getter
// answering a non-callable refused at the first step instead of at
// construction.

function twoFrom(base: number) {
  return function () {
    let i = 0;
    return { next: () => (i < 2 ? { value: base + i++, done: false } : { value: undefined, done: true }) };
  };
}

// the getter runs at concat(), once, not at the first step
const o1: any = {};
let n = 0;
Object.defineProperty(o1, Symbol.iterator, { get() { n++; console.log("GET"); return twoFrom(0); } });
console.log("before");
const it1 = Iterator.concat(o1, [7, 8]);
console.log("after concat, runs:", n); // GET before this line
console.log([...it1].join(","), "runs:", n); // 0,1,7,8 runs: 1

// a non-callable @@iterator refuses at construction, not at the
// first step
console.log("before2");
try {
  const bad: any = { [Symbol.iterator]: 5 };
  const it2 = Iterator.concat(bad);
  console.log("after concat2");
  console.log([...it2].join(","));
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
} // caught true, with no "after concat2"

// a throwing getter propagates from concat()
try {
  const o3: any = {};
  Object.defineProperty(o3, Symbol.iterator, { get() { throw new Error("boom"); } });
  Iterator.concat(o3);
  console.log("not reached");
} catch (e: any) {
  console.log("caught", e.message);
} // caught boom

// a getter answering nullish refuses too
try {
  const o4: any = {};
  Object.defineProperty(o4, Symbol.iterator, { get() { return undefined; } });
  Iterator.concat(o4);
  console.log("not reached");
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
} // caught true

// the parked method is used per item, in order, and only when its
// turn comes
const o5: any = {};
const o6: any = {};
Object.defineProperty(o5, Symbol.iterator, { get() { console.log("GET5"); return twoFrom(10); } });
Object.defineProperty(o6, Symbol.iterator, { get() { console.log("GET6"); return twoFrom(20); } });
const it5 = Iterator.concat(o5, o6);
console.log("both getters already ran");
console.log([...it5].join(",")); // 10,11,20,21

// the shapes the lazy lane still owns keep working
function* g() { yield 1; yield 2; }
console.log([...Iterator.concat(g(), [3], new Set([4]))].join(",")); // 1,2,3,4
console.log([...Iterator.concat()].join(",")); // (empty)
const m = new Map([["k", 1]]);
console.log([...Iterator.concat(m)].map((e: any) => e[0]).join(",")); // k

// a data-property @@iterator is untouched
const o7: any = { [Symbol.iterator]: twoFrom(30) };
console.log([...Iterator.concat(o7)].join(",")); // 30,31
