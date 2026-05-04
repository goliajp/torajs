// Phase 1c.1: re.exec + capturing groups in s.match.
// Uncaptured slots map to NULL pointers (see runtime_regex.c
// __torajs_regex_exec for the rationale on undefined-vs-null
// sentinel choice). Direct console.log on those slots prints
// "null" vs bun's "undefined" — a narrow shape divergence; this
// conformance case avoids printing slots that may be uncaptured.

const r1 = /a(b)c/;
const m1 = r1.exec("abc");
console.log(m1.length);
console.log(m1[0]);
console.log(m1[1]);

const r2 = /(\w+)@(\w+\.\w+)/;
const m2 = r2.exec("user@example.com");
console.log(m2.length);
console.log(m2[0]);
console.log(m2[1]);
console.log(m2[2]);

// s.match without g returns spec-shape array
const m3 = "abc".match(/a(b)c/);
console.log(m3.length);
console.log(m3[0]);
console.log(m3[1]);

// Nested groups — outer captures full repeats, inner captures last iter
const r4 = /((ab)+)c/;
const m4 = r4.exec("ababc");
console.log(m4.length);
console.log(m4[0]);
console.log(m4[1]);
console.log(m4[2]);

// s.match with g — captures stripped, only whole-match strings
const m5 = "ab1cd2".match(/(\w)(\d)/g);
console.log(m5.length);
console.log(m5[0]);
console.log(m5[1]);

// Phase 1a/1b regression — non-capturing group still works
const r6 = /(?:ab)+/;
console.log(r6.test("ababab"));
const m6 = r6.exec("xababy");
console.log(m6[0]);
