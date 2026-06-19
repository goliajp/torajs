// ES §22.1.3.{8,13,14,21,22} (String) and §23.1.3.{14,17,18} (Array)
// — `searchElement` / `searchString` is read from the argument list
// at step 1 of each algorithm; when omitted it defaults to
// `undefined`. Pre-fix tora strict-rejected 0-arg with
// "expected 1 argument(s), got 0".

// String — bun reads undefined, ToString = "undefined", searches.
// "abc" doesn't contain "undefined", so all yield -1 / false.
console.log("abc".indexOf());        // -1
console.log("abc".includes());        // false
console.log("abc".lastIndexOf());     // -1
console.log("abc".startsWith());      // false
console.log("abc".endsWith());        // false

// String haystack that contains "undefined" — verifies the
// 0-arg form truly searches for the literal, not a hard-coded -1.
console.log("undefined".indexOf());          // 0
console.log("undefined".includes());         // true
console.log("undefined".startsWith());       // true
console.log("undefined".endsWith());         // true
console.log("xundefinedy".indexOf());        // 1
console.log("xundefinedy".lastIndexOf());    // 1

// Typed Array<number> — undefined can't strict-equal any number,
// so the result is fixed regardless of contents.
console.log([1, 2, 3].indexOf());     // -1
console.log([1, 2, 3].includes());    // false
console.log([1, 2, 3].lastIndexOf()); // -1

// Typed Array<string>.
console.log(["a", "b"].indexOf());     // -1
console.log(["a", "b"].includes());    // false
console.log(["a", "b"].lastIndexOf()); // -1

