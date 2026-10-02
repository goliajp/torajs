// RegExp v flag — properties of strings (\p{RGI_Emoji_*}) compiled
// as one shared-prefix trie per class instead of one alternation
// branch per sequence. Each property, a union with a code-point
// class, the six-way RGI_Emoji union, and the early error on
// negation.

const flags = "🇧🇪🇯🇵🇺🇸";
console.log(/^\p{RGI_Emoji_Flag_Sequence}+$/v.test(flags), flags.match(/\p{RGI_Emoji_Flag_Sequence}/gv)!.length);
console.log(/^\p{RGI_Emoji_Flag_Sequence}+$/v.test("🇧🇪x"), /\p{RGI_Emoji_Flag_Sequence}/v.test("🇧"));

console.log("x👍🏽y".match(/\p{RGI_Emoji_Modifier_Sequence}/v)![0], /^\p{RGI_Emoji_Modifier_Sequence}$/v.test("👍"));
console.log("👋🏻👋🏿".match(/\p{RGI_Emoji_Modifier_Sequence}/gv)!.length);

console.log(/^\p{RGI_Emoji_ZWJ_Sequence}$/v.test("👨‍👩‍👧"), /^\p{RGI_Emoji_ZWJ_Sequence}$/v.test("👨‍👩"));
console.log("a👩🏽‍🚀b".match(/\p{RGI_Emoji_ZWJ_Sequence}/v)![0].length);

console.log(/^\p{RGI_Emoji_Tag_Sequence}$/v.test("🏴󠁧󠁢󠁥󠁮󠁧󠁿"), /^\p{RGI_Emoji_Tag_Sequence}$/v.test("🏴"));

console.log("a1️⃣b#️⃣".match(/\p{Emoji_Keycap_Sequence}/gv)!.length, /^\p{Emoji_Keycap_Sequence}$/v.test("1"));

// union with a code-point class
console.log("ab🇯🇵cd".match(/[\p{RGI_Emoji_Flag_Sequence}a-z]+/v)![0], /^[\p{RGI_Emoji_Flag_Sequence}a-z]+$/v.test("ab🇯cd"));

// the six-way union: single code points and sequences in one class
console.log(/^\p{RGI_Emoji}$/v.test("👍🏽"), /^\p{RGI_Emoji}$/v.test("👍"), /^\p{RGI_Emoji}$/v.test("🇯🇵"));
console.log("hi 👨‍👩‍👧 and 🏴󠁧󠁢󠁥󠁮󠁧󠁿!".match(/\p{RGI_Emoji}/gv)!.length);

// a quantified sequence class re-picks per iteration; the longer
// sequence wins over its prefix
console.log("👨‍👩‍👧👨‍👩‍👧‍👦".match(/\p{RGI_Emoji_ZWJ_Sequence}/gv)!.length, "👨‍👩‍👧‍👦".match(/\p{RGI_Emoji_ZWJ_Sequence}/v)![0].length);

// captures through the Pike VM
{
  const m = /(\p{RGI_Emoji_Modifier_Sequence})(\p{RGI_Emoji_Flag_Sequence})/v.exec("👍🏽🇯🇵")!;
  console.log(m[1].length, m[2].length);
}

// a property of strings cannot be negated
for (const src of ["[^\\p{RGI_Emoji_Flag_Sequence}]", "\\P{RGI_Emoji_Flag_Sequence}", "[^\\q{ab}]"]) {
  try {
    new RegExp(src, "v");
    console.log("ok");
  } catch (e) {
    console.log(e instanceof SyntaxError);
  }
}
