#!/usr/bin/env tr console.log("code right after U+2028 runs");
// ES §12.3 gives LineTerminator four members — LF, CR, U+2028,
// U+2029 — and a hashbang comment ends at any of them. The call
// above sits immediately after the separator with no newline between
// them, so a scanner that only knows LF and CR swallows it and this
// fixture prints one line instead of two. A comment there would have
// made the test vacuous: it is skipped either way.
console.log("and the rest of the program is ordinary text");
