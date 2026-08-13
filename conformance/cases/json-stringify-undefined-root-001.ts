// ES §25.5.2 step 12 — a top-level `undefined` or callable
// serializes to NOTHING, so the CALL answers the undefined value, not
// text. SSA folds `undefined` into the same pointer slot as `null`,
// which DOES print "null", and the top-level arm took that fold at
// face value: `JSON.stringify(undefined)` answered the string "null".
// A callable had no static walk at all and was a loud reject.
console.log(JSON.stringify(undefined));
console.log(String(JSON.stringify(undefined)));
console.log("" + JSON.stringify(undefined));
console.log(String(JSON.stringify(function () { return 1; })));
const fn2 = function () {};
console.log(String(JSON.stringify(fn2)));
console.log(String(JSON.stringify(undefined, null, 2)));

// With a FUNCTION replacer the root is still offered to it and can
// come back as anything — the undefined answer is not a shortcut that
// may be taken early.
console.log(String(JSON.stringify(undefined, function (k: string, v: any) {
  return 42;
})));
console.log(String(JSON.stringify(function () {}, function (k: string, v: any) {
  return 42;
})));

// An ARRAY replacer names no property of a non-object, so the root
// still answers undefined.
console.log(String(JSON.stringify(undefined, ["a"])));

// Inside a composite the same value keeps the other two verdicts of
// the §25.5.2 three-way split: an object key is omitted, an array
// slot becomes `null`.
console.log(JSON.stringify({ a: undefined }));
console.log(JSON.stringify([undefined]));

// And a top-level `null` is still the string "null".
console.log(JSON.stringify(null));
