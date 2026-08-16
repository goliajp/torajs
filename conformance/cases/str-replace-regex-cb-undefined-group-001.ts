// §22.2.6.11 step 14.g — a capture group that did not participate in
// the match is `undefined` when the replacer function reads it, not
// the empty String. tr built an empty Str for that slot, so
// `"xz".replace(/x(y)?(z)/, (m, p1) => "<" + p1 + ">")` answered `<>`
// where every engine answers `<undefined>` — silently, which is the
// outcome the design principles rank worst.
//
// The fix is the cell the `match` / `exec` array lanes have pushed for
// this case all along: the immortal undefined sentinel, whose payload
// reads back as "undefined" through every Str consumer and whose drop
// is a no-op because it carries FLAG_STATIC_LITERAL.
//
// The `$1` expansion in the STRING-replacement lane is deliberately
// not this case: §22.2.6.11's GetSubstitution really does substitute
// "" for an undefined capture, and the last line pins that difference.

// an optional group that the match skipped
console.log("xz".replace(/x(y)?(z)/, function (m: string, p1: string, p2: string) {
  return "<" + p1 + "|" + p2 + ">";
}));

// the same pattern with the group present
console.log("xyz".replace(/x(y)?(z)/, function (m: string, p1: string, p2: string) {
  return "<" + p1 + "|" + p2 + ">";
}));

// an alternation leaves the branch that lost out non-participating
console.log("abc".replace(/(a)|(q)/, function (m: string, p1: string, p2: string) {
  return p1 + "/" + p2;
}));

// global walk — the skipped group is per-match, not per-call
console.log("a1 b a2".replace(/([ab])(\d)?/g, function (m: string, p1: string, p2: string) {
  return p1 + ":" + p2 + " ";
}));

// §22.2.6.11 GetSubstitution — "" for the string-replacement lane
console.log("xz".replace(/x(y)?(z)/, "[$1$2]"));

// and the array lanes agree with the callback lane
console.log(JSON.stringify("abc".match(/(a)|(q)/)));
console.log(JSON.stringify(/x(y)?(z)/.exec("xz")));
