// `s.replace(re, callback)` over a subject past ASCII: the callback
// sees the match, the groups, the offset in UTF-16 code units and the
// subject, and what it returns is spliced in whatever its encoding.
// The callback is held in a variable, so the walk runs in the runtime
// kernel; each pattern is run as a literal and through `new RegExp`.

const bracket = (m: string): string => "[" + m + "]";
const offsets = (m: string, off: number): string => "<" + m + "@" + off + ">";
const groups = (m: string, a: string, b: string, off: number, input: string): string =>
  b + "=" + a + "(" + off + "/" + input.length + ")";
const wide = (m: string): string => "日" + m.length;
const narrow = (m: string): string => "" + m.length;
const drop = (m: string): string => "";
const loose: any = (m: string, off: number): string => "{" + m + off + "}";

function both(s: string, a: RegExp, b: RegExp, f: (m: string) => string): void {
  console.log(s.replace(a, f));
  console.log(s.replace(b, f));
}

// Latin-1 subject
both("aééb é", /é+/g, new RegExp("é+", "g"), bracket);
both("aééb é", /é+/, new RegExp("é+"), bracket);
both("aééb é", /é+/g, new RegExp("é+", "g"), wide);
both("aééb é", /x*/g, new RegExp("x*", "g"), bracket);
console.log("aééb é".replace(/é+/g, offsets));
console.log("aééb é".replace(new RegExp("é+", "g"), offsets));
console.log("clé=valeur; é=ü".replace(/([^=; ]+)=([^; ]*)/g, groups));
console.log("clé=valeur; é=ü".replace(new RegExp("([^=; ]+)=([^; ]*)", "g"), groups));

// UTF-16 subject
both("x日y日日z", /日+/g, new RegExp("日+", "g"), bracket);
both("x日y日日z", /日+/g, new RegExp("日+", "g"), narrow);
both("日é日", /日/g, new RegExp("日", "g"), drop);
both("a😀😀b😀", /😀+/gu, new RegExp("😀+", "gu"), bracket);
both("a😀b", /x*/gu, new RegExp("x*", "gu"), bracket);
both("a😀b", /x*/g, new RegExp("x*", "g"), narrow);
both("  Héllo 日本語 42", /\p{L}+/gu, new RegExp("\\p{L}+", "gu"), bracket);
console.log("x日y日日z".replace(/日+/g, offsets));
console.log("x日y日日z".replace(new RegExp("日+", "g"), offsets));
console.log("鍵=値; k=v".replace(/([^=; ]+)=([^; ]*)/g, groups));
console.log("鍵=値; k=v".replace(new RegExp("([^=; ]+)=([^; ]*)", "g"), groups));
console.log("x日y日日z".replace(/日+/g, loose));
console.log("aééb é".replace(/é+/g, loose));

// sticky without `g`: one exec anchored at lastIndex
const sticky = /日+/y;
sticky.lastIndex = 3;
console.log("x日y日日z".replace(sticky, bracket), sticky.lastIndex);
console.log("x日y日日z".replace(sticky, bracket), sticky.lastIndex);
