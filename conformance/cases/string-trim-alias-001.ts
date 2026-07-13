// RFC 20260713-string-proto-residual blade 1 — Annex B §B.2.3
// trimLeft / trimRight aliases: the property values ARE the
// trimStart / trimEnd functions (reference identity, name/length
// reflect the canonical function). Typed tier already carried the
// alias; this covers the any tier's intern.

// any-tier calls
var s: any = "  hi  ";
console.log("[" + s.trimLeft() + "]");
console.log("[" + s.trimRight() + "]");

// typed-tier calls (regression)
console.log("[" + "  ok  ".trimLeft() + "]");
console.log("[" + "  ok  ".trimRight() + "]");

// reference identity per §B.2.3.1/2
console.log("left is start:", String.prototype.trimLeft === String.prototype.trimStart);
console.log("right is end:", String.prototype.trimRight === String.prototype.trimEnd);

// reflection answers the canonical face
console.log("name:", String.prototype.trimLeft.name);
console.log("len:", String.prototype.trimRight.length);
