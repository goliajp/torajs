// RC-4 replace A1_T4 — functional replaceValue for STRING patterns
// (ES §22.1.3.18 step 10 / §22.1.3.19). Before this lane the closure
// was passed raw into __torajs_str_replace's third Str param and the
// runtime deref'd it as a Str block (SIGSEGV). The callback runs
// through the closure's boxed entry: argv is [matched, position,
// whole] padded with undefined, so any declared arity works.

// 1-param callback (matched)
console.log("abc".replace("b", function (m) { return "X"; }));

// position arg (test262 A1_T4 shape: a2 + "")
console.log("gnulluna".replace("null", function (a1, a2, a3) { return a2 + ""; }));

// 2-param callback, first occurrence only
console.log("abcabc".replace("b", function (m, pos) { return "" + pos; }));

// replaceAll walks every occurrence with per-match position
console.log("a-b-c".replaceAll("-", function (m, pos) { return "" + pos; }));

// empty pattern: insert at every position incl. end-of-string
console.log("ab".replaceAll("", function (m, pos) { return "[" + pos + "]"; }));

// declared params beyond (matched, position, whole) read undefined
console.log("abc".replace("b", function (m, pos, whole, extra) { return "" + extra; }));

// UTF-16 replacement widens the Latin-1 haystack
console.log("abc".replace("b", function (m) { return "é中"; }));

// no match returns the receiver unchanged
console.log("abc".replace("z", function (m) { return "X"; }));

// matched text flows through concat
console.log("xyx".replaceAll("x", function (m) { return m + "!"; }));

// a throw inside the callback aborts the walk and propagates
function thrower(): void {
  "abc".replace("b", function (m): string { throw new Error("cb-throw"); });
  console.log("unreachable");
}
try {
  thrower();
} catch (e) {
  console.log("caught");
}
