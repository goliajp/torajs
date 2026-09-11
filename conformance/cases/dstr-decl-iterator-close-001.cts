// A hand-written iterator object driven by array destructuring: the
// protocol re-dispatches next() / return() inside the runtime, so
// nothing in the program names the struct method family — the
// dispatch judgment stubbed it and the first next() hit the loud
// reject. The judgment now keeps the obj world whenever the
// iteration protocol is emitted.
var returnCalled = 0;
var iterable: any = {};
iterable[Symbol.iterator] = function () {
  return {
    next() {
      return { done: false };
    },
    return() {
      returnCalled++;
      return {};
    },
  };
};
var [a, b] = iterable;
console.log("ab", a, b, returnCalled);
var [c] = iterable;
console.log("c", c, returnCalled);
var [] = iterable;
console.log("empty", returnCalled);
