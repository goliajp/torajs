// A `number`-returning function with a path that runs off the end of
// its body.
//
// ES §10.2.1.4 [[Call]] step 11: a body that completes normally
// answers `undefined`. tr asserted that path unreachable instead, so
// `g(false)` killed the process outright — exit 133, SIGTRAP, with no
// output and no error. Not a wrong answer: no answer at all.
//
// An I64 slot has no bit pattern left to mean "undefined", but F64
// does, and the sentinel plus its consumers (typeof / strict-eq /
// print / boxing) were already in service for out-of-range reads and
// `find` misses. So the return slot of exactly these functions widens
// to F64 and the tail writes the sentinel; a function whose every
// path returns keeps the slot it had.

function annotated(f: boolean): number {
  if (f) {
    return 7;
  }
}

function inferred(f: boolean) {
  if (f) {
    return 7;
  }
}

const hit = annotated(true);
const miss = annotated(false);

console.log(hit, miss);
console.log(typeof hit, typeof miss);
console.log(miss === undefined, miss == undefined, miss === 7);
console.log(miss + 1);
console.log(String(miss));
console.log(inferred(true), inferred(false));

// boxing into the any world, and reading it back
const boxed: any = annotated(false);
console.log(boxed);
console.log(typeof boxed);

// a value that fell through, passed on
function relay(n: number): number {
  return n;
}
console.log(relay(annotated(true)));

// every path returns — untouched
function full(f: boolean): number {
  if (f) {
    return 1;
  }
  return 2;
}
console.log(full(true), full(false), full(false) + 1);

// both arms return — also untouched
function both(f: boolean): number {
  if (f) {
    return 1;
  } else {
    return 2;
  }
}
console.log(both(true), both(false), both(true) * 10);

// a throw closes its path just like a return does
function throwing(f: boolean): number {
  if (f) {
    return 1;
  }
  throw new Error("no");
}
console.log(throwing(true));

// loops are not analysed, so this one widens even though it always
// returns — the answer stays right either way
function looping(n: number): number {
  while (true) {
    return n * 2;
  }
}
console.log(looping(4));

// a `return` inside a `try` hands its value to the finally tail
// instead of returning directly, and that path used to skip the
// return-value coercion entirely — a `return 1` landed an I64 constant
// in the widened slot. The tail's "did someone return" flag also has
// to start zeroed, which only matters once the fall-through path is
// reachable at all.
function guarded(f: boolean): number {
  try {
    if (f) {
      return 1;
    }
  } finally {
    console.log("cleanup");
  }
}
console.log(guarded(true));
console.log(guarded(false));

// an exception raised in a try with no catch leaves the function, so
// the body alone decides whether this one falls through
function escapes(f: boolean): number {
  try {
    return f ? 1 : 2;
  } finally {
    console.log("out");
  }
}
console.log(escapes(true), escapes(false));

// with a catch, both halves have to return
function handled(f: boolean): number {
  try {
    if (f) {
      return 1;
    }
  } catch (e) {
    return 9;
  }
}
console.log(handled(true), handled(false));

// switch without a default can miss every case
function switched(n: number): number {
  switch (n) {
    case 1:
      return 10;
    case 2:
      return 20;
  }
}
console.log(switched(1), switched(2), switched(3));
console.log(typeof switched(3));
