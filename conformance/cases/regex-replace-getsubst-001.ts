// GetSubstitution regex-needle: $` (before match) / $' (after match) /
// $<name> (named captures) — ES §22.1.3.18.5, test262 S15.5.4.11_A2 系
// + RegExp/named-groups string-replace 面.
const str = "She sells seashells by the seashore.";

// $' non-global (A2_T10) / global (A2_T5)
console.log(str.replace(/sh/, "$'" + "sch"));
console.log(str.replace(/sh/g, "$'" + "sch"));

// $` non-global (A2_T9) / global (A2_T4)
console.log(str.replace(/sh/, "$`" + "sch"));
console.log(str.replace(/sh/g, "$`" + "sch"));

// $` / $& / $' combined
console.log("abcd".replace(/bc/, "<$`|$&|$'>"));

// $<name> participating group
console.log("2026-07-14".replace(/(?<y>\d{4})-(?<m>\d{2})-(?<d>\d{2})/, "$<d>/$<m>/$<y>"));

// $<name> unknown name → empty (pattern has named groups)
console.log("abcd".replace(/(?<fst>.)(?<snd>.)/, "[$<fth>]"));

// $<name> unparticipating alternation arm → empty
console.log("abcd".replace(/(?<fst>.)(?<snd>.)|(?<thd>x)/, "[$<thd>]"));

// no named groups at all → $<x> literal
console.log("abcd".replace(/bc/, "[$<x>]"));

// unterminated $< → literal
console.log("abcd".replace(/(?<fst>.)/, "[$<fst]"));

// $$ / $& regression
console.log("abcd".replace(/bc/, "$$|$&"));

// replaceAll shares expand_repl
console.log("a-b-c".replaceAll(/-/g, "[$`]"));
