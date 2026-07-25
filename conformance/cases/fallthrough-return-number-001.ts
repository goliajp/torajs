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
