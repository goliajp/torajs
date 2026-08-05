// The `.then` / `.catch` repr word describes the HANDLER, so it has
// to be read off the handler — not off the bits-ABI adapter that may
// have been wrapped around it.
//
// A handler whose negotiated signature carries an f64 face gets
// wrapped in a synthesized `(i64) -> i64` thunk so the value crosses
// the promise runtime's fixed calling convention in the right
// register bank. The repr word handed to the kernel was computed from
// that wrapper, which names neither end of the handler inside it, so
// two facts were lost whenever the wrap fired:
//
//   - an `any` parameter stopped being marked as one, and the kernel
//     skipped the boxing such a handler needs — every value arrived
//     one DOUBLE_ENCODE_OFFSET short of itself, silently;
//   - the callback leg's result was stamped "integer" whatever the
//     handler really returned, so a downstream `any` reader boxed raw
//     f64 bits, or a string pointer, as an integer.
//
// Both halves are below, plus typed-lane lines that must not move.

// an `any` parameter, wrapped only because the return is f64-faced
Promise.resolve(12.5).then((v: any) => {
  console.log("any-param", v);
  return 0;
});

// the same shape with the return annotated rather than inferred
Promise.resolve(12.5).then((v: any): number => {
  console.log("any-param-ann", v);
  return 0;
});

// no value return: never wrapped, so this always worked
Promise.resolve(12.5).then((v: any) => {
  console.log("no-return", v);
});

// the return half — a typed f64 handler read downstream through `any`
Promise.resolve(1.5)
  .then((v: number) => v * 2)
  .then((w: any) => {
    console.log("f64-ret", w);
  });

// the return half again, this time a string out of an f64-param
// handler: the wrap fires for the PARAMETER, and the stamp used to
// call the Str pointer an integer
Promise.resolve(1.5)
  .then((v: number) => (v > 1 ? "big" : "small"))
  .then((w: any) => {
    console.log("str-ret", w);
  });

// both legs of the two-arg form
Promise.resolve(2.5).then(
  (v: any) => {
    console.log("then2-ok", v);
    return 1.5;
  },
  (e: any) => {
    console.log("then2-unused", e);
    return 0.5;
  },
);
Promise.reject(7.5).then(
  (v: any) => {
    console.log("then2-unused", v);
    return 1.5;
  },
  (e: any) => {
    console.log("then2-err", e);
    return 0.5;
  },
);

// catch
Promise.reject(3.5).catch((e: any) => {
  console.log("catch", e);
  return 2.5;
});

// finally takes no value, so only its return half exists
Promise.resolve(4.5).finally(() => {
  console.log("finally");
});

// a captured local, to keep the closure-shaped handler covered
const k = 100;
Promise.resolve(5.5)
  .then((v: any) => {
    console.log("closure", v, k);
    return v;
  })
  .then((w: any) => {
    console.log("closure-chain", w);
  });

// typed lanes, which the repr word also feeds — these must not move
Promise.resolve(6.5).then((v: number) => {
  console.log("typed-f64", v);
  return 0;
});
Promise.resolve(7).then((v: number) => {
  console.log("typed-int", v);
  return 0;
});
Promise.resolve("s").then((v: string) => {
  console.log("typed-str", v);
  return 0;
});
