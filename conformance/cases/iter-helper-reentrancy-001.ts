// §27.5.3.2 GeneratorValidate step 6 — an Iterator Helper is a spec
// generator: a re-entrant next()/return() (called from inside the
// callback or inner iterator the current step is driving) throws a
// catchable TypeError instead of recursing (test262 Iterator/concat
// throws-typeerror-when-generator-is-running-next was a
// stack-overflow SIGSEGV before the executing gate).
let enterCount = 0;
let testIterator = {
  next() {
    enterCount++;
    iterator.next();
    return { done: false };
  },
};
let iterable = {
  [Symbol.iterator]() {
    return testIterator;
  },
};
let iterator = Iterator.concat(iterable);
try {
  iterator.next();
} catch (e) {
  console.log("next reentry", e instanceof TypeError);
}
console.log(enterCount); // 1

// return() from inside a running step is the same validate
let retIterator = {
  next() {
    iterator2.return();
    return { done: false, value: 1 };
  },
};
let iterable2 = {
  [Symbol.iterator]() {
    return retIterator;
  },
};
let iterator2 = Iterator.concat(iterable2);
try {
  iterator2.next();
} catch (e) {
  console.log("return reentry", e instanceof TypeError);
}

// after the throw the helper is still usable (running flag cleared)
let iterator3 = Iterator.concat([10]);
console.log(iterator3.next().value); // 10
console.log(iterator3.next().done); // true
