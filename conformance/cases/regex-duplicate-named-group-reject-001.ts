// ES §22.2.1.1 GroupSpecifier — a duplicate GroupSpecifier `name`
// within a single pattern is a Static Semantics early error. bun /
// spec throw SyntaxError from the constructor; pre-fix tr silently
// accepted the second definition (silently shadowing).

try { new RegExp("(?<a>x)(?<a>y)", ""); console.log("no-throw"); }
catch (e: any) { console.log("dup:", e.name); }

try { new RegExp("(?<x>a)(?<x>b)", "u"); console.log("no-throw u"); }
catch (e: any) { console.log("dup-u:", e.name); }

try { new RegExp("(?<same>a)(?<same>b)(?<same>c)", ""); console.log("no-throw tri"); }
catch (e: any) { console.log("dup-tri:", e.name); }

// Distinct named groups still parse + match cleanly.
const r = new RegExp("(?<year>\\d+)-(?<month>\\d+)", "");
console.log("distinct-test:", r.test("2026-07"));
console.log("distinct-source:", r.source);

// Single named group still works.
const r2 = new RegExp("(?<only>abc)", "");
console.log("single:", r2.test("abc"));
console.log("single-source:", r2.source);

// Positional captures unaffected — `(a)(a)` is fine (only names dup).
const r3 = new RegExp("(a)(a)", "");
console.log("positional-dup:", r3.test("aa"));
