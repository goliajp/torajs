// Phase 1c.4.c: positional back-references `\1`..`\9` resolve to the
// captured slice; per-thread `br_offset` state machine consumes the
// backref's bytes one at a time in the outer match loop.

// Single-char backref.
console.log(/(\d)\1/.test("11"));
console.log(/(\d)\1/.test("12"));

// Multi-char backref.
console.log(/(abc)\1/.test("abcabc"));
console.log(/(abc)\1/.test("abcabd"));

// Variable-width capture + backref.
console.log(/(\w+)\s\1/.test("foo foo"));
console.log(/(\w+)\s\1/.test("foo bar"));

// Backref of an empty (non-participating) capture matches anywhere.
console.log(/(a)|(b)\2/.test("a"));
console.log(/^(?:(a)|(b))\2$/.test("ba"));

// Backref + i-flag (case-fold via char_eq; receiver must be `.` etc
// since [a-z]/i is pre-existing OP_CLASS i-flag limitation).
console.log(/(.)\1/i.test("Aa"));
console.log(/(.)\1/i.test("Ab"));

// Two backrefs in sequence.
console.log(/(\w)\1\1/.test("aaa"));
console.log(/(\w)\1\1/.test("aab"));

// Backref inside alternation.
{
  const m = "abxab".match(/(\w+)x\1/);
  console.log(m === null ? "null" : m[0]);
  console.log(m === null ? "null" : m[1]);
}

// .test() + .replace with backref pattern.
console.log("abab".replace(/(\w+)/, "<$1>"));
