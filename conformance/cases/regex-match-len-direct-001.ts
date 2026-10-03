// a match only null-tested or measured, on a literal pattern without
// g / y, walks the literal's own compiled matcher at the call site
// when the haystack is ascii. every other haystack, a regexp rebuilt
// by compile(), a rebound name and the g / y flags take the runtime
// entry; all of them must answer what the full match answers.
const word = /^\w+/;
const digits = /\d+/;
const pair = /(\w)(\w+)/;
const empty = /x*/;
const ci = /hello/i;
const glob = /o+/g;
const sticky = /o/y;
const swapped = /a+/;

function probe(s: string): string {
  const a = s.match(word);
  const b = s.match(digits);
  const c = s.match(pair);
  const d = s.match(empty);
  const e = s.match(ci);
  const f = s.match(/r+l/);
  return [
    a !== null ? a[0].length : -1,
    b !== null ? b[0].length : -1,
    c !== null ? c[0].length : -1,
    d !== null ? d[0].length : -1,
    e !== null ? e[0].length : -1,
    f !== null ? f[0].length : -1,
  ].join(",");
}

console.log(probe("hello world 42"));
console.log(probe("  HeLLo 7"));
console.log(probe(""));
console.log(probe("héllo wörld 42"));
console.log(probe("中文 world 9"));
console.log(probe(String.fromCharCode(119, 111, 114, 108, 100, 49)));
console.log(probe("hello".toUpperCase() + " " + (12345).toString()));

// lastIndex is neither read nor written without g / y
function plain(s: string): string {
  digits.lastIndex = 3;
  const m = s.match(digits);
  return (m !== null ? m[0].length : -1) + " " + digits.lastIndex;
}
console.log(plain("ab 123"));
console.log(plain("ab"));

// g: the first hit's length, lastIndex reset to 0
function withG(s: string): string {
  glob.lastIndex = 5;
  const m = s.match(glob);
  return (m !== null ? m[0].length : -1) + " " + glob.lastIndex;
}
console.log(withG("foo boo"));
console.log(withG("xyz"));

// y: one attempt at lastIndex, which moves on a hit and resets on a miss
function stick(s: string): string {
  const m = s.match(sticky);
  return (m !== null ? m[0].length : -1) + " " + sticky.lastIndex;
}
sticky.lastIndex = 1;
console.log(stick("foo"));
console.log(stick("foo"));
console.log(stick("foo"));

// a rebound name is not a literal
function bound(s: string): string {
  let re = /a+/;
  const m = s.match(re);
  re = /b+/;
  const n = s.match(re);
  return (m !== null ? m[0].length : -1) + " " + (n !== null ? n[0].length : -1);
}
console.log(bound("baaa bb"));
console.log(bound("xbbb"));

// compile() rebuilds the regexp in place
function rebuilt(s: string): number {
  const m = s.match(swapped);
  return m !== null ? m[0].length : -1;
}
console.log(rebuilt("xaay bbb"));
swapped.compile("b+");
console.log(rebuilt("xaay bbb"));
console.log(rebuilt("xaay"));

function loop(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const s = "row " + i.toString() + (i % 3 === 0 ? " é" : " e");
    const m = s.match(digits);
    if (m !== null) total = total + m[0].length;
    const w = s.match(word);
    if (w) total = total + 100;
    const p = ("中" + s).match(pair);
    if (p !== null) total = total + 1000 * p[0].length;
  }
  return total;
}
console.log(loop(30));
