// Substr VIEW vs owned-Str equality across encodings -- a view
// inherits its parent encoding and cannot narrow, so a UTF-16
// view whose surviving units are all Latin-1 must still compare
// equal to the canonical Latin-1 literal (per-code-unit walk,
// not the canonical-encoding short-circuit).
// RFC 20260712-string-proto-cluster chunk A2.

// slice off the wide unit -> all-ascii utf-16 view.
console.log("\u6C49abc".slice(1) === "abc");
console.log("abc" === "\u6C49abc".slice(1));
console.log("\u6C49abc".slice(1) === "abd");
console.log("\u6C49abc".substring(1) === "abc");

// same-encoding views keep working.
console.log("\u6C49abc".slice(0, 1) === "\u6C49");
console.log("xyabc".slice(2) === "abc");

// substr trim produces a view -- trimmed-to-ascii content must
// compare equal to the latin-1 literal.
const s = "xx\u00A0hi\u3000yy";
console.log(s.slice(2, 6).trim() === "hi");

// dynamic (runtime-computed) compares through the any lane.
const parts: any = "\u6C49-ok".split("-");
console.log(parts[1] === "ok");
