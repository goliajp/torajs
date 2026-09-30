// The arms of an alternation rejoin one state: a looped alternation of
// strings (a property of strings under `v` expands to one) builds a
// matcher whose size follows the pattern, and answers what the dynamic
// engine answers.

function both(lit: RegExp, src: string, flags: string, s: string): void {
  const dyn = new RegExp(src, flags);
  const a = lit.exec(s);
  const b = dyn.exec(s);
  console.log(a === null ? "null" : a[0].length, b === null ? "null" : b[0].length);
}

const arms = /^(?:ab|cd|e)+$/;
for (const s of ["abcde", "eab", "abc", "", "eeee", "cdab"]) {
  both(arms, "^(?:ab|cd|e)+$", "", s);
}

// both arms claim `a` and rejoin with the same slots
const anchored = /(?:^a|a)b/;
for (const s of ["ab", "xab", "xa", "b"]) {
  both(anchored, "(?:^a|a)b", "", s);
}

const many = /^(?:あい|うえ|おか|きく|けこ|さし|すせ|そた|x)+$/;
for (const s of ["あいxそた", "あう", "xけこけ", ""]) {
  both(many, "^(?:あい|うえ|おか|きく|けこ|さし|すせ|そた|x)+$", "", s);
}

const keycaps = /^\p{Emoji_Keycap_Sequence}+$/v;
for (const s of ["1️⃣#️⃣", "1️", "1️⃣x", ""]) {
  both(keycaps, "^\\p{Emoji_Keycap_Sequence}+$", "v", s);
}
