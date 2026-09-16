// §13.15.5.5 — an element's target REFERENCE is evaluated before that
// element's step. A throw there leaves [[done]] false, so the pattern
// still owes the iterator an IteratorClose, called with no arguments.
let nextCount = 0;
let returnCount = 0;
let thisIsIterator = false;
let returnArgc = -1;
const iterator: any = {
  next: function () {
    nextCount += 1;
    return { done: true, value: undefined };
  },
  return: function (this: any) {
    returnCount += 1;
    thisIsIterator = this === iterator;
    returnArgc = arguments.length;
    return {};
  },
};
const iterable: any = {};
iterable[Symbol.iterator] = function () {
  return iterator;
};
function thrower(): string {
  throw new Error("lref");
}
let caught = "none";
try {
  [(({}) as any)[thrower()]] = iterable;
} catch (e: any) {
  caught = e.message;
}
console.log(nextCount, returnCount, thisIsIterator, returnArgc, caught);
