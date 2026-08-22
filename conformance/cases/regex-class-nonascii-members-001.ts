// Character classes hold code points, not the bytes of their own
// UTF-8 encoding: a non-ASCII member has to match the character it
// spells, and `\uHHHH` inside a class is a code point in every mode.
const probes: string[] = [
  JSON.stringify("日本語のテキスト".match(/テ[キク]スト/u)),
  JSON.stringify("日本語のテキスト".match(/[日月]本/u)),
  JSON.stringify("aéc".match(/a[é日X]c/u)),
  JSON.stringify("a日c".match(/a[é日X]c/u)),
  JSON.stringify("aXc".match(/a[é日X]c/u)),
  JSON.stringify("Ã".match(/a[é日X]c/u)),
  JSON.stringify("あ".match(/[ぁ-ん]/u)),
  JSON.stringify("é".match(/[a-\xFF]/u)),
  JSON.stringify("A".match(/[\u{41}]/u)),
  JSON.stringify("A".match(/[A]/u)),
  JSON.stringify("A".match(/[\u{41}]/)),
  JSON.stringify("日".match(/[^X]/u)),
  JSON.stringify("日".match(/[\p{L}]/u)),
  JSON.stringify("キ".match(/[キク]/v)),
  JSON.stringify("abc".match(/a[bc]c/)),
];
for (const p of probes) console.log(p);
