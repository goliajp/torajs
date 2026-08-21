// rotation 470 — the dead-start skip must not change which position a
// match starts at. Each shape below is one of the reasons the skip
// declines or has to look wider than the plain start state.

// zero-width: an entry state that accepts empty matches everywhere,
// including before a byte with no transition out of the start state
console.log(JSON.stringify("xyz".match(/a*/)));
console.log(JSON.stringify("...abc".match(/a*/g)));
console.log("qqq".replace(/x*/g, "-"));

// word boundaries pick a mid entry per the left-byte class, so the
// skip has to admit a byte any of the three entries can take
console.log(JSON.stringify("foo bar baz".match(/\bbar\b/)));
console.log(JSON.stringify("xbar bar".match(/\bbar/)));
console.log(JSON.stringify("abc123".match(/\B\d+/)));

// multiline / anchored: `^` blocks the mid entries entirely
console.log(JSON.stringify("aa\nbb\ncc".match(/^bb$/m)));
console.log(JSON.stringify("zzz".match(/^a/)));

// non-ASCII haystack under u flag — the skip is off here because a
// landing position must stay on a code-point boundary
console.log(JSON.stringify("日本語のテキスト".match(/(テ|ハ)キスト/u)));
console.log(JSON.stringify("αβγ hello".match(/\p{L}+/u)));
console.log(JSON.stringify("naïve café".match(/caf./u)));

// case-insensitive: the entry admits both cases of the first letter
console.log(JSON.stringify("before HELLO world".match(/hello/i)));
console.log(JSON.stringify("HeLLo hello".match(/hello/gi)));

// global iteration keeps advancing from lastIndex, so the skip runs
// from a non-zero start too
const re = /ab/g;
let hits = 0;
let m: string[] | null = re.exec("zzabzzabzz");
while (m !== null) {
  hits = hits + 1;
  m = re.exec("zzabzzabzz");
}
console.log(hits);

// no match at all — the scan must walk off the end and answer null
console.log("aaaa".match(/b/));
console.log(JSON.stringify("aaaa".match(/b*/)));

// match at the very end, including zero-width at end of input
console.log(JSON.stringify("hello world".match(/world$/)));
console.log(JSON.stringify("abc".match(/$/)));
console.log(JSON.stringify("abc".split(/(?:)/u)));
