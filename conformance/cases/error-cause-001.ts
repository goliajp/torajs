// ES2022 §20.5.8.1 InstallErrorCause — `new Error(msg, { cause })`.
// Pre-fix the second argument was accepted and silently dropped, so
// every shape below read `undefined` while every engine reads the
// value back.
//
// The test is HasProperty, not a defined value: `{ cause: undefined }`
// owns the property, `{}` does not. `options.cause !== undefined`
// cannot tell those two apart, which is why the check is spelled with
// `in`.
//
// Enumerability was deliberately not asserted when this was written
// (every Error own property was enumerable then); that gap has since
// closed, so the assertions live at the bottom now — the ctor-installed
// `cause` is own but non-enumerable (§20.5.8.1's
// CreateNonEnumerableDataPropertyOrThrow), like `message`.

const strCause = new Error("m", { cause: "c" });
console.log(String(strCause.cause));

const numCause = new Error("m", { cause: 42 });
console.log(String(numCause.cause));

// an Error as the cause — the common wrap-and-rethrow shape
const inner = new Error("inner");
const wrapped = new Error("outer", { cause: inner });
console.log((wrapped.cause as any).message);

// HasProperty, not value-defined
console.log("cause" in new Error("m"));
console.log("cause" in new Error("m", {}));
console.log("cause" in new Error("m", { cause: undefined }));

// every NativeError subclass forwards `options` to Error's ctor
console.log(String(new TypeError("t", { cause: "tc" }).cause));
console.log(String(new RangeError("r", { cause: "rc" }).cause));
console.log(String(new SyntaxError("s", { cause: "sc" }).cause));
console.log(String(new ReferenceError("rf", { cause: "rfc" }).cause));

// AggregateError carries its data params ahead of message + options
const agg = new AggregateError([new Error("x")], "am", { cause: "ac" });
console.log(agg.errors.length, agg.name, String(agg.cause));
console.log("cause" in new AggregateError([], "am"));

// a user subclass passing the options bag straight through
class Wrapped extends Error {
  constructor(m: string, opts?: any) {
    super(m, opts);
    this.name = "Wrapped";
  }
}
const w = new Wrapped("wm", { cause: "wc" });
console.log(w.name, w.message, String(w.cause));
console.log("cause" in new Wrapped("wm"));

// a non-Object options argument installs nothing and must not throw —
// `in` would have thrown a TypeError had the Object guard not run first
console.log(String(new Error("m", 5 as any).message));
console.log(String(new Error("m", "s" as any).message));
console.log(String(new Error("m", null as any).message));
console.log("cause" in new Error("m", 5 as any));

// the value survives a throw / catch round trip
try {
  throw new TypeError("thrown", { cause: "why" });
} catch (e: any) {
  console.log(e.name, e.message, String(e.cause));
}

// enumerability (see header note): ctor-installed slots are own but
// non-enumerable, so none of them appear in keys / JSON
console.log(Object.keys(new Error("m", { cause: "c" })).length);
console.log(JSON.stringify(new Error("m", { cause: "c" })));
console.log(new Error("m", { cause: "c" }).hasOwnProperty("cause"));
