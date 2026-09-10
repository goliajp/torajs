// A `return` inside a for-of / for-in / switch body is a return like
// any other. The pair of walks that decide a function's inferred
// return type — "does this body return a value at all" and "what
// type" — both stopped at those statements, so the function typed
// itself Void and the return was rejected ("function expects Void,
// got Number"). The same loop written C-style or as a `while`
// inferred Number, because those arms exist.
const it = [1, 2];
console.log(
  (function () {
    for (const x of it) {
      return x + 33;
    }
    throw new Error("unreachable");
  })(),
);
console.log(
  (function () {
    for (const k in { a: 1 }) {
      return k;
    }
    throw new Error("unreachable");
  })(),
);
console.log(
  (function () {
    try {
      for (const x of it) {
        return x * 2;
      }
    } finally {
    }
    throw new Error("unreachable");
  })(),
);
console.log(
  (function (n: number) {
    switch (n) {
      case 1:
        return "one";
      default:
        return "other";
    }
  })(1),
);
// A body whose only returns sit in these shapes but are bare keeps
// the Void reading.
console.log(
  (function () {
    for (const x of it) {
      return;
    }
  })(),
);
