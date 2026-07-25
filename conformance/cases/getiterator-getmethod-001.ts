// RFC 20260725-getiterator-getmethod 刀 2 — §7.4.2 GetIterator asks
// the receiver for its `@@iterator` property instead of looking up a
// mangled method name in the class vtable.
//
// Before this, only a class instance could be iterable, and only
// because the parser had folded its `[Symbol.iterator]()` member into
// a `__sym_Symbol_iterator__` method. An object literal declaring the
// same member iterated nothing.

function counter(n: number): any {
  let i = 0;
  return {
    next() {
      const d = i >= n;
      const v = d ? 0 : i;
      i = i + 1;
      return { value: v, done: d };
    },
  };
}

// Data-property form.
const a: any = { [Symbol.iterator]: function () { return counter(3); } };
const seenA: number[] = [];
for (const v of a) seenA.push(v as number);
console.log(seenA.join(","));

// Method-shorthand form — its body used to be dropped at parse time.
const b: any = { [Symbol.iterator]() { return counter(3); } };
console.log([...b].join(","));

// Assigned after the fact, and inherited off a user prototype: the
// lookup is an ordinary property walk, so both work.
const c: any = {};
c[Symbol.iterator] = function () { return counter(2); };
console.log([...c].join(","));

const proto: any = { [Symbol.iterator]() { return counter(4); } };
const d: any = Object.create(proto);
console.log([...d].join(","));

// A user `@@iterator` OUTRANKS the builtin lane — this is what makes
// patching one mean anything.
const arr: any = [1, 2, 3];
arr[Symbol.iterator] = function () { return counter(2); };
console.log([...arr].join(","), arr.length);

// §7.4.2 step 3 — present but NOT callable is a TypeError, not a
// silent fallback to some other way of iterating.
const bad: any = {};
bad[Symbol.iterator] = 5;
try {
  for (const v of bad) console.log("unreachable", v);
} catch (err: any) {
  console.log("not-callable:", err instanceof TypeError);
}

// An `@@iterator` that throws forwards the abrupt completion.
const boom: any = { [Symbol.iterator]() { throw new Error("boom"); } };
try {
  for (const v of boom) console.log("unreachable", v);
} catch (err: any) {
  console.log("threw:", err.message);
}

// A `next()` that throws does too.
const boom2: any = {
  [Symbol.iterator]() {
    return { next() { throw new Error("step-boom"); } };
  },
};
try {
  for (const v of boom2) console.log("unreachable", v);
} catch (err: any) {
  console.log("step-threw:", err.message);
}

// §7.4.4 IteratorComplete is ToBoolean(done), not an identity test
// against `true`.
const truthy: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        i = i + 1;
        return i > 2 ? { value: 0, done: 1 } : { value: i, done: 0 };
      },
    };
  },
};
console.log([...truthy].join(","));

// A missing `value` reads as undefined per §7.4.5.
const novalue: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        i = i + 1;
        return i > 2 ? { done: true } : { done: false };
      },
    };
  },
};
const seenU: string[] = [];
for (const v of novalue) seenU.push(String(v));
console.log(seenU.join(","));

// §7.4.9 IteratorClose — a destructuring pattern that stops before
// the iterator reports done owes it a `return()` call, and `return`
// is an ordinary property here just like `next` is.
let closed = "no";
const closable: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        i = i + 1;
        return { value: i, done: false };
      },
      return() {
        closed = "yes";
        return { value: 0, done: true };
      },
    };
  },
};
const [first] = closable;
console.log("first:", first, "closed:", closed);

// An iterator with no `return` closes silently rather than throwing.
const noreturn: any = {
  [Symbol.iterator]() {
    let i = 0;
    return { next() { i = i + 1; return { value: i, done: false }; } };
  },
};
const [only] = noreturn;
console.log("only:", only);

// Nothing above disturbed the builtin lanes.
console.log([..."ab"].join(","), [...[7, 8]].join(","));
const m = new Map<string, number>();
m.set("k", 1);
for (const [k, v] of m) console.log(k, v);
function* gen() { yield 1; yield 2; }
const gseen: number[] = [];
for (const v of gen()) gseen.push(v);
console.log(gseen.join(","));
