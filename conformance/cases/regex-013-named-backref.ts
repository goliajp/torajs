// Phase 1c.4.c: named back-references `\k<name>` resolve to the same
// capture slot as the positional backref via the parser's name table;
// forward references work because resolution defers to a post-parse
// walk over the AST.

// Single named backref.
console.log(/(?<d>\d)\k<d>/.test("11"));
console.log(/(?<d>\d)\k<d>/.test("12"));

// Multi-char named backref.
console.log(/(?<word>\w+)\s\k<word>/.test("foo foo"));
console.log(/(?<word>\w+)\s\k<word>/.test("foo bar"));

// Two named groups, two backrefs.
console.log(/(?<a>\d)(?<b>\d)\k<a>\k<b>/.test("1212"));
console.log(/(?<a>\d)(?<b>\d)\k<a>\k<b>/.test("1221"));

// Forward reference (named ref resolved post-parse).
console.log(/(?<a>x)\k<a>/.test("xx"));

// Mixed positional + named — both reference the same group.
console.log(/(?<g>\w)\1\k<g>/.test("aaa"));
console.log(/(?<g>\w)\1\k<g>/.test("aab"));

// .match with named-group pattern — positional captures still present.
{
  const m = "John Doe".match(/(?<first>\w+)\s(?<last>\w+)/);
  console.log(m === null ? "null" : m[0]);
  console.log(m === null ? "null" : m[1]);
  console.log(m === null ? "null" : m[2]);
}

// Named backref under i-flag (via `.` since OP_CLASS i-flag pending).
console.log(/(?<x>.)\k<x>/i.test("Aa"));
console.log(/(?<x>.)\k<x>/i.test("Ab"));
