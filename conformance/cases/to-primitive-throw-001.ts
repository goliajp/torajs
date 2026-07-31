// OrdinaryToPrimitive both-methods-answer-objects raises a catchable
// TypeError through Number()/String() over an Any receiver (§7.1.4 /
// §7.1.17 step 3). Pre-fix the pending throw was stranded (no
// emit_throw_check after the coercion kernels): NaN leaked out of
// Number(), the Test262Error branch ran in String() cases, and the
// stranded throw poisoned the next runtime entry — a SECOND
// Number(x) SIGSEGVed on a placeholder rc_dec (test262
// Number/S8.12.8_A4, String/S8.12.8_A1).

let x: any = {
  valueOf: function () {
    return new Object();
  },
  toString: function () {
    return new Object();
  },
};

// first call throws, catchable
try {
  Number(x);
  console.log("no throw");
} catch (e: any) {
  console.log("num caught:", e instanceof TypeError);
}

// second call must throw again (not SIGSEGV)
try {
  Number(x);
  console.log("no throw 2");
} catch (e: any) {
  console.log("num caught 2:", e instanceof TypeError);
}

// String() over the same receiver: display kernel, same TypeError
try {
  String(x);
  console.log("str no throw");
} catch (e: any) {
  console.log("str caught:", e instanceof TypeError);
}

// single-method receivers keep the non-throwing fallback lanes
let y: any = {
  valueOf: function () {
    return new Object();
  },
};
console.log("fallback num:", Number(y)); // toString → "[object Object]" → NaN
console.log("fallback str:", String(y)); // toString builtin

// valueOf answering a primitive keeps the fast path
let z: any = {
  valueOf: function () {
    return 42;
  },
  toString: function () {
    return new Object();
  },
};
console.log("prim num:", Number(z));
