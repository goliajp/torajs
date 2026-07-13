// LineContinuation (ES §12.9.4.3): `\` + LineTerminatorSequence in a
// string/template literal contributes nothing to the value.
// test262 trim/15.5.4.20-4-1 shape.
var s = "	a b\
c 	";
console.log(s.trim());
console.log(s.length);

// double-quoted, LF
const a = "one\
two";
console.log(a, a.length);

// single-quoted
const b = 'x\
y';
console.log(b);

// template literal
const t = `p\
q`;
console.log(t, t.length);

// escaped-backslash before newline is NOT a continuation: `\\` is a
// real backslash, then the raw newline stays (multiline template).
const u = `m\\
n`;
console.log(u.length);

// continuation chained twice
const c = "1\
2\
3";
console.log(c);
