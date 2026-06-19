// ES §22.1.3.16.5 StringPad step 6: if filler is the empty String,
// return S. Previously tora's pad fell back to U+0020 space fill
// (C-port carry-over) and produced "  abc" for `"abc".padStart(5, "")`.
console.log("abc".padStart(5, ""));
console.log("abc".padEnd(5, ""));
console.log("abc".padStart(10, ""));
console.log("abc".padEnd(10, ""));
console.log("".padStart(5, ""));
console.log("".padEnd(5, ""));
// Length <= original still passthrough (orthogonal to empty filler).
console.log("abc".padStart(3, ""));
console.log("abc".padStart(2, ""));
// Default fill (no second arg) is " ", unchanged.
console.log("abc".padStart(5));
console.log("abc".padEnd(5));
// Non-empty fill, unchanged.
console.log("abc".padStart(5, "x"));
console.log("abc".padEnd(5, "xy"));
