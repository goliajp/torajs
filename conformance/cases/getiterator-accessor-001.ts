// §7.4.2 GetIterator's GetMethod(obj, @@iterator) is a real Get, so an
// accessor-shaped @@iterator has to run its getter. The symbol probe
// answers an ACCESSOR sentinel; this lane read it as a VALUE — neither
// undefined nor null, so it slipped past the nullish gate into
// callable_entry, which boxed it into something no closure lookup
// recognises. Every accessor spelling threw "Symbol.iterator is not a
// function" with the getter never running.
//
// The class-declared `get [Symbol.iterator]()` spelling is NOT covered
// here: the parser folds a class `[Symbol.iterator]` member into a
// mangled vtable method, and the accessor spelling loses the property
// entirely (getOwnPropertyDescriptor answers none) — a separate gap.

function twoFrom(base: number) {
  return function () {
    let i = 0;
    return { next: () => (i < 2 ? { value: base + i++, done: false } : { value: undefined, done: true }) };
  };
}

// own accessor, installed dynamically
const o1: any = {};
Object.defineProperty(o1, Symbol.iterator, {
  get() { console.log("GET own"); return twoFrom(0); },
});
console.log([...o1].join(",")); // GET own / 0,1

// object-literal getter syntax
const o2: any = { get [Symbol.iterator]() { return twoFrom(10); } };
console.log([...o2].join(",")); // 10,11

// inherited accessor
const proto: any = {};
Object.defineProperty(proto, Symbol.iterator, { get() { return twoFrom(20); } });
console.log([...(Object.create(proto) as any)].join(",")); // 20,21

// a throwing getter propagates instead of being reported as
// "not a function"
const o3: any = {};
Object.defineProperty(o3, Symbol.iterator, { get() { throw new Error("boom"); } });
try {
  console.log([...o3]);
} catch (e: any) {
  console.log("caught", e.message);
} // caught boom

// a getter answering a non-callable still refuses (the getter runs
// first, which is the whole point)
const o4: any = {};
Object.defineProperty(o4, Symbol.iterator, { get() { console.log("GET o4"); return 5; } });
try {
  console.log([...o4]);
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
} // GET o4 / caught true

// exactly one getter run per GetIterator, across every consumer
const o5: any = {};
let n = 0;
Object.defineProperty(o5, Symbol.iterator, { get() { n++; return twoFrom(30); } });
const [a, b] = o5;
console.log(a, b, n); // 30 31 1
console.log(Array.from(o5).join(","), n); // 30,31 2
function* g() { yield* o5; }
console.log([...g()].join(","), n); // 30,31 3

// data-property and builtin forms are untouched
const o7: any = { [Symbol.iterator]: twoFrom(50) };
console.log([...o7].join(",")); // 50,51
console.log([...[1, 2]].join(",")); // 1,2

// the async twin walks the same GetMethod
const o6: any = {};
Object.defineProperty(o6, Symbol.asyncIterator, {
  get() {
    console.log("GET async");
    return function () {
      let i = 0;
      return { next: () => Promise.resolve(i < 2 ? { value: 40 + i++, done: false } : { value: undefined, done: true }) };
    };
  },
});
async function drain() {
  const out: any[] = [];
  for await (const x of o6) out.push(x);
  console.log(out.join(","));
}
drain(); // GET async / 40,41
