// §13.15.5.5 AssignmentRestElement — the rest target's reference is
// evaluated BEFORE the iterator is drained. A target that throws
// therefore closes the iterator (return() runs once) and never
// drains it; the slice-of-a-prefetched-array shape drained first,
// which on a never-done iterator never came back.
var returnCalled = 0;
var iterable: any = {};
iterable[Symbol.iterator] = function () {
  return {
    next() {
      return { done: false, value: 1 };
    },
    return() {
      returnCalled++;
      return {};
    },
  };
};
function throwlhs(): any {
  throw "in lhs";
}
try {
  0, [...({} as any)[throwlhs()]] = iterable;
  console.log("BAD: no throw");
} catch (e) {
  console.log("threw", e, "returnCalled", returnCalled);
}

// A finite rest drain still lands where it did: the target is
// evaluated first, then the drain fills it.
const log: string[] = [];
const holder: any = {};
function key(): string {
  log.push("key");
  return "rest";
}
const src: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        log.push("next");
        return i < 2 ? { value: i++, done: false } : { value: undefined, done: true };
      },
    };
  },
};
[holder[key()]] = [7];
[...holder[key()]] = src;
console.log(holder.rest.join(","), log.join(","));

// A member target rooted at a plain binding still widens that
// binding: `y` lands on `x` itself, not on an alias of it.
var x: any = {};
var result: any;
var vals = [4, 3, 2];
result = [...x.y] = vals;
console.log(x.y.length, x.y.join(","), result === vals);
