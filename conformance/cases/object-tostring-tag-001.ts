// §20.1.3.6 steps 15-16 — Object.prototype.toString asks the receiver
// for its @@toStringTag and keeps the answer when it is a String.
const T: any = Symbol.toStringTag;
const ts: any = Object.prototype.toString;

// own tag, written and defined
const written: any = {};
written[T] = "Written";
console.log(ts.call(written));

const defined: any = {};
Object.defineProperty(defined, T, { value: "Defined", configurable: true });
console.log(ts.call(defined));

// step 16 — a non-String tag falls back to the builtinTag
const numeric: any = {};
numeric[T] = 42;
console.log(ts.call(numeric));

const undef: any = {};
undef[T] = undefined;
console.log(ts.call(undef));

// no tag at all keeps the builtinTag walk intact
console.log(ts.call({ a: 1 }));
console.log(ts.call([1, 2]));
console.log(ts.call(null));
console.log(ts.call(undefined));

// step 15 is a full Get — an INHERITED tag counts
const parent: any = {};
parent[T] = "Inherited";
const child: any = Object.create(parent);
console.log(ts.call(child));

// the tag wins over the builtinTag, not the other way round
const arr: any = [1, 2, 3];
arr[T] = "NotAnArray";
console.log(ts.call(arr));

// deleting the tag restores the builtinTag
const gone: any = {};
gone[T] = "Temporary";
console.log(ts.call(gone));
delete gone[T];
console.log(ts.call(gone));

// a tag that is itself built by concatenation (exercises the
// variable-length badge builder rather than a short literal)
const long: any = {};
long[T] = "A" + "VeryLongCustomToStringTagValue";
console.log(ts.call(long));

// the empty string is still a String
const empty: any = {};
empty[T] = "";
console.log(ts.call(empty));
