// RFC 20260718-builtin-error-ctor-first-class 刀 3 — ES2025 §20.5.2.1
// Error.isError: true exactly for [[ErrorData]] carriers (tr: the
// FLAG_ERROR header bit every injected-error factory stamps), false
// for plain objects and every primitive. Rides the ordinary
// static-method pipeline, so the reflection face (typeof) comes from
// the reified own entry.
console.log(Error.isError(new Error("x")));
console.log(Error.isError(new TypeError("y")));
console.log(Error.isError({}));
console.log(Error.isError("Error"));
console.log(Error.isError(null));
console.log(Error.isError(undefined));
console.log(Error.isError(5));
console.log(typeof Error.isError);
class MyE extends Error {}
console.log(Error.isError(new MyE("z")));
