// RegExp v flag — chunk B3: properties of strings (\p{RGI_Emoji}
// family). CODEGEN tables from UCD 17.0 emoji-sequences /
// emoji-zwj-sequences; single-cp members ride the cp set, multi-cp
// sequences join the class strings and desugar like \q{…}.

// standalone escape
console.log(
  /^\p{RGI_Emoji}$/v.test("😀"),
  /^\p{RGI_Emoji}$/v.test("🇧🇪"),
  /^\p{RGI_Emoji}$/v.test("x")
);
console.log(/^\p{Emoji_Keycap_Sequence}$/v.test("1️⃣"), /^\p{Emoji_Keycap_Sequence}$/v.test("1"));
console.log(/^\p{Basic_Emoji}$/v.test("☃️"), /^\p{Basic_Emoji}$/v.test("⌚"));
console.log(
  /^\p{RGI_Emoji_ZWJ_Sequence}$/v.test("👨‍👩‍👧"),
  /^\p{RGI_Emoji_Flag_Sequence}$/v.test("🇯🇵"),
  /^\p{RGI_Emoji_Modifier_Sequence}$/v.test("👍🏽"),
  /^\p{RGI_Emoji_Tag_Sequence}$/v.test("🏴󠁧󠁢󠁥󠁮󠁧󠁿")
);
// inside a class + set algebra with \q and cp classes
console.log(/^[\p{Emoji_Keycap_Sequence}]$/v.test("#️⃣"), /^[[0-9]\p{Emoji_Keycap_Sequence}]$/v.test("5"));
console.log(/^[\p{RGI_Emoji}--\q{😀}]$/v.test("😀"), /^[\p{RGI_Emoji}--\q{😀}]$/v.test("😁"));
console.log(/^[\p{Basic_Emoji}&&\p{RGI_Emoji}]$/v.test("⌚"));
// global scan mixes single-cp and sequence members
const g = "😀🇯🇵x1️⃣".match(/\p{RGI_Emoji}/gv);
console.log(g ? g.length : 0);
// non-emoji text stays clean
console.log(/\p{RGI_Emoji}/v.test("plain ascii text"), /\p{Basic_Emoji}/v.test("π∑"));
