// v0.3 #3.b: process.env.NAME — runtime getenv. Returns
// Nullable<String>; missing var → NULL (semantic equivalent to bun's
// `undefined` thanks to tr's undefined→null bridge). The conformance
// case asserts via `=== undefined` rather than printing the missing
// value directly (tr would print "null", bun "undefined" — the
// `===` comparison agrees on undefined-vs-set, but `null !== undefined`
// in bun while tr collapses them; assertions here use only the
// agree-on-both shape).

const home = process.env.HOME;
console.log(home === null);
console.log(home === undefined);

const missing = process.env.NONEXISTENT_TR_TEST_VAR___1234;
console.log(missing === undefined);
