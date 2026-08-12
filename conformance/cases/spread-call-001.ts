// §13.3.8.1 ArgumentListEvaluation — dynamic spread in call arguments
// (rotation 372): the runtime spread lane materializes the full list
// into one Array<Any> and reads argc off it.

// objlit method + multiple spreads + trailing comma (the t262
// arguments-object family shape)
const arr = [2, 3];
const obj = {
  method() {
    console.log(arguments.length, arguments[0], arguments[1], arguments[2], arguments[3]);
  }
};
obj.method(42, ...[1], ...arr,);

// IIFE + spread
(function () {
  console.log("iife", arguments.length, arguments[2]);
})(0, ...[1, 2]);

// closure-value arguments-style callees: length-only face + argv face
const f = function () {
  return arguments.length;
};
const xs = [2, 3];
console.log("cv", f(1, ...xs));
console.log("mid", f(...xs, 9));
console.log("empty", f(...[]));

const g4 = function () {
  console.log("argv", arguments.length, arguments[0], arguments[1]);
};
const anyArr: any[] = [7, 8];
g4(5, ...anyArr);

// string source iterates per code point
console.log("str", f(..."ab"), f(..."a\u{1F44B}"));

// Set source
console.log("set", f(...new Set([1, 2, 3])));

// custom [Symbol.iterator] — values arrive in order
const it: any = {};
it[Symbol.iterator] = function () {
  let n = 0;
  return {
    next: function () {
      n += 1;
      return n <= 3 ? { value: n * 10, done: false } : { value: undefined, done: true };
    }
  };
};
const h = function () {
  console.log("cust", arguments.length, arguments[1], arguments[3]);
};
h(5, ...it);

// custom iterator whose next() throws — §7.4.5 IteratorStep error
// propagates catchably
const bad: any = {};
bad[Symbol.iterator] = function () {
  return { next: function () { throw new Error("boom"); } };
};
try {
  (function () {})(0, ...bad);
} catch (e) {
  console.log("caught", (e as Error).message);
}

// spreading a non-iterable is a catchable TypeError
try {
  f(...(5 as any));
} catch (e) {
  console.log("notiter", e instanceof TypeError);
}
