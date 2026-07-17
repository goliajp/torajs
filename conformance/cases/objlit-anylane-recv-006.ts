// RFC 20260717-objlit-anylane-recv knife 2e — a recv-first closure
// handed to a HOF as the callback binds this = undefined (no-thisArg
// callbacks, §23.1.3.19 etc). Pre-fix the raw boxed ABI fed the
// element into __this, so `this.v` read off the element instead of
// throwing (arr.map(o.f) answered NaN NaN where bun throws).

const o: any = { v: 100, f(x) { return this.v + x; } };
const arr: any = [1, 2, 3];

function expectThrow(tag, run) {
  try {
    run();
    console.log(tag, "no throw");
  } catch (e) {
    console.log(tag, e instanceof TypeError);
  }
}

expectThrow("map:", () => arr.map(o.f));
expectThrow("filter:", () => arr.filter(o.f));
expectThrow("forEach:", () => arr.forEach(o.f));
expectThrow("find:", () => arr.find(o.f));
expectThrow("every:", () => arr.every(o.f));
expectThrow("reduce:", () => arr.reduce(o.f, 0));
expectThrow("sort:", () => {
  const s: any = [3, 1, 2];
  s.sort(o.f);
});

const st: any = new Set([1, 2]);
expectThrow("setForEach:", () => st.forEach(o.f));
const mp: any = new Map([["k", 1]]);
expectThrow("mapForEach:", () => mp.forEach(o.f));

// plain (this-free) callbacks keep working on every lane
const plain: any = (x: any) => x * 2;
console.log(arr.map(plain).join(",")); // 2,4,6
console.log(arr.filter((x: any) => x > 1).join(",")); // 2,3
console.log(arr.reduce((a: any, b: any) => a + b, 0)); // 6
console.log([3, 1, 2].sort((a, b) => a - b).join(",")); // 1,2,3
console.log("done");
