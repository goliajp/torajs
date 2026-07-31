// Iterator helpers on TYPED receivers — RFC 20260730-iterator-global
// §3.3 SSA face. The receiver types as a concrete iterator shape
// (generator ClassRef / extends-Iterator heir / ArrIter / MapIter)
// and the helper call rides the any-lane dispatcher.

function* g() {
  yield 1;
  yield 2;
  yield 3;
}

// lazy adapters straight off the generator factory's return
console.log(g().map((x: any) => x * 2).toArray());
console.log(g().filter((x: any) => x % 2 === 1).toArray());
console.log(g().drop(1).take(1).toArray());
console.log(g().flatMap((x: any) => [x, x]).toArray());

// eager consumers
console.log(g().reduce((a: any, b: any) => a + b, 10));
console.log(g().find((x: any) => x > 1));
console.log(g().some((x: any) => x === 3));
console.log(g().every((x: any) => x > 0));
let acc = 0;
g().forEach((x: any) => {
  acc += x;
});
console.log(acc);

// ArrIter / MapIter receivers
console.log([10, 20, 30].values().map((x: any) => x + 1).toArray());
const m = new Map([[1, "a"], [2, "b"]]);
console.log(m.keys().drop(1).toArray());
console.log(m.values().toArray());

// extends-Iterator user class, plus a grandchild through the chain
class Counter extends Iterator {
  i = 0;
  next() {
    this.i += 1;
    return { done: this.i > 4, value: this.i * 100 };
  }
}
console.log(new Counter().take(2).toArray());

class Skipper extends Counter {}
console.log(new Skipper().drop(2).toArray());
