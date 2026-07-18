// rotation 140 — Error.prototype.toString (§20.5.3.4) generic
// receiver + abrupt completions. test262 undefined-props /
// tostring-get-throws / tostring-message-throws-symbol /
// tostring-message-throws-toprimitive: the desugar rewrote
// `Error.prototype.toString.call(x)` to `x.toString()` (the badge),
// and the runtime cell carried the generic TO_STRING mid. Now the
// rewrite SKIPS the Error/toString pair, Error.prototype's own
// `toString` entry carries the dedicated ANY_METHOD_ERROR_TO_STRING
// cell, and its dispatch runs the spec's Get(name)/Get(message)
// steps (abrupt Get / abrupt ToString propagate; undefined defaults
// "Error" / "").

// Generic plain-object receivers (the spec's own test matrix).
console.log(Error.prototype.toString.call({}));
console.log(Error.prototype.toString.call({ message: "42" }));
console.log(Error.prototype.toString.call({ name: "24" }));
console.log(Error.prototype.toString.call({ name: "24", message: "42" }));

// Empty-name special case: bare message, no colon.
console.log(Error.prototype.toString.call({ name: "", message: "m" }));

// Real error instances still ride the fast lane.
console.log(Error.prototype.toString.call(new Error("x")));
console.log(Error.prototype.toString.call(new TypeError("t")));

// ToString(name) runs a user toString; numbers coerce.
console.log(Error.prototype.toString.call({ name: { toString() { return "N"; } }, message: 7 }));

// Abrupt: a throwing name getter propagates.
try {
  Error.prototype.toString.call({ get name() { throw new RangeError("g"); } });
} catch (e: any) {
  console.log("get-throw:", e instanceof RangeError, e.message);
}

// Abrupt: ToString(message) through a throwing toPrimitive.
try {
  Error.prototype.toString.call({ name: "ok", message: { toString() { throw new RangeError("tp"); } } });
} catch (e: any) {
  console.log("toprim-throw:", e instanceof RangeError, e.message);
}

// Non-object receiver throws TypeError (step 2).
try {
  (Error.prototype.toString as any).call(5);
} catch (e: any) {
  console.log("non-obj:", e instanceof TypeError);
}

// Reified through a variable — same cell identity both read paths.
const f: any = (Error.prototype as any).toString;
console.log("var-form:", f.call({ name: "V" }), f === (Error.prototype as any).toString);
