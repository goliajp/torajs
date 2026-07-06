// RC-4 multiline-`$` face (RFC 20260706-test262-bug-corpus) — under
// the m flag `$` asserts before every `\n` (spec §22.2.2.4), but the
// DFA's ε-closure resolves AnchorE only at text end (PositionCtx has
// no right byte), so `/s$/m` on "pairs\nmakes" silently missed.
// Multiline `$` patterns are gated off the DFA (can_dfa) onto the
// Pike VM, whose AnchorE is m-aware. test262 S15.10.2.6_A1_T{3,4}
// flip to true pass.

console.log(/s$/m.test("pairs\nmakes\tdouble"));
let r = /s$/m.exec("pairs\nmakes\tdouble");
if (r !== null) {
  console.log(r.index, r[0]);
}

// Global multiline scan finds every line end.
console.log("a1\nb2\nc3".match(/\d$/gm));

// ^ (already m-aware in the DFA) keeps working alongside.
console.log(/^m/m.test("pairs\nmakes"));

// Non-m `$` stays text-end-only (DFA path untouched).
console.log(/s$/.test("pairs\nmakes"));
console.log(/pairs$/.test("pairs\nmakes"));
