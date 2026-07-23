// `new RegExp(malformed, flags)` throws a catchable SyntaxError.
// Pre-fix tr silently returned a never-match rejected stub. bun / spec
// (§22.2.3.1 RegExpCreate → CompilePattern → Parse Pattern) throws
// SyntaxError from the constructor.
try { new RegExp("\\u{ZZZ}", "u"); } catch (e: any) { console.log("u-bad-esc:", e.name); }
try { new RegExp("[", ""); } catch (e: any) { console.log("unmatched-class:", e.name); }
try { new RegExp("(unbalanced", ""); } catch (e: any) { console.log("unmatched-paren:", e.name); }
try { new RegExp("a", "uv"); } catch (e: any) { console.log("uv-conflict:", e.name); }

// Valid patterns compile cleanly + are useable.
const ok = new RegExp("a+b", "");
console.log("ok:", ok.test("aaab"));
console.log("ok-source:", ok.source);
console.log("ok-flags:", ok.flags);

// annexB-recoverable non-u patterns keep the existing lenient path
// (rejected=0, no throw). `\u{ZZZ}` in non-u mode is an identity
// escape `u`+`{ZZZ}` per §B.1.4 — accepts and never matches
// `abc`.
const lenient = new RegExp("\\u{ZZZ}", "");
console.log("lenient-test:", lenient.test("abc"));
console.log("lenient-source:", lenient.source);

// Catch-and-continue — post-throw execution keeps working.
let caught = 0;
for (const bad of ["[", "(", "\\u{ZZZ}"]) {
  try {
    new RegExp(bad, "u");
  } catch {
    caught++;
  }
}
console.log("caught-count:", caught);
