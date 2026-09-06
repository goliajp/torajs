// §22.1.3.19 calls a functional replaceValue as `Call(replaceValue,
// undefined, «matched, …, string»)`, and so does the regexp route it
// delegates to (§22.2.6.11 step 14.a). Nothing in either spelling
// depends on how the receiver was written down, so the two spellings
// of one program have to answer the same. They did not: the receiver
// test was `Expr::String(_)`, so a literal ran and a `const` holding
// that same literal was an unclaimed-receiver refusal.
const S = function (m: string): string {
  const th = this as any;
  return "<" + m + (th === undefined ? "u" : "o") + ">";
};

// The spelling that already worked.
console.log("xQx".replace("Q", S));
console.log("xQx".replace(/Q/, S));

// The same receiver through a binding.
const s = "xQx";
console.log(s.replace("Q", S));
console.log(s.replace(/Q/, S));

const g = "xQxQ";
console.log(g.replace(/Q/g, S));
console.log(g.replaceAll("Q", S));

// Written in place rather than reached through a name — the slot is
// admitted either way.
console.log(s.replace("Q", function (m: string): string {
  const th = this as any;
  return "[" + m + (th === undefined ? "u" : "o") + "]";
}));

// A replacer that reads no `this` was never affected; it is here so a
// regression that stops admitting the slot shows up as a diff rather
// than as this file quietly testing nothing.
console.log(s.replace("Q", function (m: string): string {
  return "(" + m + ")";
}));
