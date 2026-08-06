// The collection constructors' iterable argument (§24.1.1.1 step 7,
// §24.2.2.1, §24.3.1.1, §24.4.1.1). Four constructors, one algorithm,
// one runtime walk.
//
// The static lanes still own an argument written as an array literal
// at the call site; everything whose shape is only known at run time —
// an `any` binding, a generator, another collection, a string, a
// nullish argument — is walked here.

// §24.1.1.1 step 6 — a nullish iterable adds nothing and is not an
// error.
const m0 = new Map(null as any);
const s0 = new Set(undefined as any);
const wm0 = new WeakMap(null as any);
const ws0 = new WeakSet(undefined as any);
console.log(m0.size, s0.size, typeof wm0, typeof ws0);

// An `any` binding — the shape the checker cannot see through.
const pairs: any = [["a", 1], ["b", 2]];
const m1 = new Map(pairs);
console.log(m1.size, m1.get("a"), m1.get("b"), m1.has("c"));

const values: any = [3, 1, 3, 2];
const s1 = new Set(values);
console.log(s1.size, s1.has(1), s1.has(3), s1.has(9));

// The weak pair takes the same initializer, with its own key rule.
const k1 = { id: 1 };
const k2 = { id: 2 };
const wmPairs: any = [[k1, "one"], [k2, "two"]];
const wm1 = new WeakMap(wmPairs);
console.log(wm1.get(k1), wm1.get(k2), wm1.has({} as any));

const wsItems: any = [k1, k2];
const ws1 = new WeakSet(wsItems);
console.log(ws1.has(k1), ws1.has(k2), ws1.has({} as any));

// §24.3.1.2 / §24.4.1.2 — a key that cannot be held weakly is a
// TypeError from the adder, not from the walk.
try {
  const bad: any = [[1, "x"]];
  new WeakMap(bad);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// A generator source — no length, no indices, just the protocol.
function* gen() {
  yield ["g", 7];
  yield ["h", 8];
}
const m2 = new Map(gen() as any);
console.log(m2.size, m2.get("g"), m2.get("h"));

// Another collection as the source (its default iterator is entries
// for a Map, values for a Set).
const m3 = new Map(new Map([["x", 1]]) as any);
console.log(m3.get("x"));
const s3 = new Set(new Set([1, 2, 3]) as any);
console.log(s3.size, s3.has(2));
const s4 = new Set(new Map([["y", 2]]) as any);
console.log(s4.size);

// A string source steps per code unit, and each character has to
// arrive as a plain string — a view of the parent would hash and
// compare against the parent's pointer instead of its text.
const s5 = new Set("hello" as any);
console.log(s5.size, s5.has("h"), s5.has("l"), s5.has("z"));
console.log([...(s5 as any)].join(""));

// §24.1.1.2 step 4.d — a Map entry that is not an Object throws.
try {
  const notEntries: any = [1, 2];
  new Map(notEntries);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// §7.4.11 — an abrupt completion inside the walk propagates, and the
// iterator is closed on the way out.
let closes = 0;
const closing: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        i++;
        return i <= 2 ? { value: [i, i], done: false } : { value: undefined, done: true };
      },
      return() {
        closes++;
        return { done: true };
      },
    };
  },
};
const m4 = new Map(closing);
console.log(m4.size, m4.get(2), closes);

const thrower: any = {
  [Symbol.iterator]() {
    return {
      next() {
        throw new RangeError("from next");
      },
    };
  },
};
try {
  new Map(thrower);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof RangeError, (e as any).message);
}
