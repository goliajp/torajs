// LengthOfArrayLike is ToLength(Get(O, "length")): a `length` accessor
// on an object literal runs, once, and a throw from it propagates.
const counted = {
  get length() {
    console.log("length read");
    return 2;
  },
  1: 7,
};
console.log([].includes.call(counted, 7));
console.log(Array.prototype.indexOf.call(counted, 7));
console.log(Array.prototype.join.call(counted, "-"));

const throwing = {
  get length(): number {
    throw new TypeError("no length");
  },
};
try {
  [].includes.call(throwing, 1);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError, (e as Error).message);
}

// A data `length` keeps answering from the field.
const data = { length: 3, 0: "a", 2: "c" };
console.log(Array.prototype.join.call(data, "+"));
