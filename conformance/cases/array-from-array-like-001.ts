// §23.1.2.1 step 3 splits on whether the source has an iterator, and
// the `no` side is not an error: `Array.from` walks `length` and the
// index keys. Reaching that side through `any` used to throw, because
// the only array-like route was chosen from the static type.

const al: any = { length: 3, 0: "a", 1: "b", 2: "c" };
console.log("array-like  :", Array.from(al).join("|"));

// absent indices are holes, and Get answers undefined for each
const holed: any = { length: 3, 0: "a", 2: "c" };
console.log("holes       :", JSON.stringify(Array.from(holed)));

// ToLength: a numeric string converts, NaN and negatives answer empty
const strLen: any = { length: "2", 0: 1, 1: 2 };
console.log("string len  :", JSON.stringify(Array.from(strLen)));
const nanLen: any = { length: "zz", 0: "a" };
console.log("NaN len     :", JSON.stringify(Array.from(nanLen)));
const negLen: any = { length: -1, 0: "a" };
console.log("negative len:", JSON.stringify(Array.from(negLen)));

// no `length` at all is ToLength(undefined) = 0, not an error
const plain: any = { a: 1 };
console.log("no length   :", JSON.stringify(Array.from(plain)));
console.log("static plain:", JSON.stringify(Array.from({ a: 1 })));

// a primitive has no iterator either, and answers the empty walk
const prim: any = 5;
console.log("number      :", JSON.stringify(Array.from(prim)));

// null and undefined still throw — they have no properties to ask
const nothing: any = null;
try {
  Array.from(nothing);
  console.log("null        : no throw");
} catch (e) {
  console.log("null        : threw");
}

// every iterable lane still takes the iterator branch
const asStr: any = "abc";
console.log("string      :", Array.from(asStr).join("|"));
const asArr: any = [1, 2, 3];
console.log("array       :", Array.from(asArr).join("|"));
const asSet: any = new Set([1, 2]);
console.log("set         :", Array.from(asSet).join("|"));
const asMap: any = new Map([["a", 1]]);
console.log("map         :", JSON.stringify(Array.from(asMap)));

function* gen() {
  yield 1;
  yield 2;
}
const asGen: any = gen();
console.log("generator   :", Array.from(asGen).join("|"));

const userIter: any = {
  [Symbol.iterator]() {
    let n = 0;
    return {
      next() {
        n = n + 1;
        return { value: n * 10, done: n > 3 };
      },
    };
  },
};
console.log("@@iterator  :", Array.from(userIter).join("|"));

// the array-like branch belongs to Array.from ALONE — spreading the
// same object is not iterating it
try {
  const spread = [...al];
  console.log("spread      : no throw", spread.length);
} catch (e) {
  console.log("spread      : threw");
}
