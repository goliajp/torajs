// rotation 460 — §19.2.5.1 step 2 ToInt32(radix) is a REAL ToNumber,
// so the radix coercion can complete abruptly: a `valueOf` that
// throws propagates, and an object whose valueOf AND toString both
// answer objects raises TypeError (§7.1.1 OrdinaryToPrimitive step
// 5). `any_to_number` records both on the throw TLS and answers NaN;
// without the throw check the parse ran on that NaN and the pending
// throw surfaced at an unrelated site — t262
// parseInt/S15.1.2.2_A3.1_T7 exited 139 instead of throwing.
var bothObjects: any = {
  valueOf: function () {
    return {};
  },
  toString: function () {
    return {};
  },
};
try {
  parseInt("11", bothObjects);
  console.log("no-throw");
} catch (e) {
  console.log("typeerror", e instanceof TypeError);
}

var valueOfThrows: any = {
  valueOf: function () {
    throw "error";
  },
  toString: function () {
    return 2;
  },
};
try {
  parseInt("11", valueOfThrows);
  console.log("no-throw");
} catch (e) {
  console.log("caught", e);
}
try {
  Number.parseInt("11", valueOfThrows);
  console.log("no-throw");
} catch (e) {
  console.log("caught-ns", e);
}

// The non-abrupt coercions keep working: valueOf wins for hint
// number, and a toString-only object still reaches the radix.
var viaValueOf: any = {
  valueOf: function () {
    return 2;
  },
  toString: function () {
    return 1;
  },
};
console.log(parseInt("11", viaValueOf));
var viaToString: any = {
  toString: function () {
    return 2;
  },
};
console.log(parseInt("11", viaToString));
console.log("end");
