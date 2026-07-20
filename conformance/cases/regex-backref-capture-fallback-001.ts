// RFC 20260721-string-proto-cluster knife 9 (G7) — Pike VM span-keyed
// dedup: shorter-capture backref candidates must survive when the
// greedy capture dies downstream.
console.log("aaaaaaaaaa,aaaaaaaaaaaaaaa".replace(/^(a+)\1*,\1+$/, "$1"));
console.log(/^(a+)\1*,\1+$/.test("aaaaaaaaaa,aaaaaaaaaaaaaaa"));
const m = "aaaaaaaaaa,aaaaaaaaaaaaaaa".match(/^(a+)\1*,\1+$/);
if (m !== null) console.log(m[1]); else console.log("no-match");
// falls all the way to length 1: 2 a's vs 3 a's — only L=1 divides both
const m2 = "aa,aaa".match(/^(a+)\1*,\1+$/);
if (m2 !== null) console.log(m2[1]); else console.log("no-match");
// truly no match: right side empty
console.log(/^(a+)\1*,\1+$/.test("aaaa,"));
// baseline backrefs must not regress
const m3 = "abcabc".match(/(abc)\1/);
if (m3 !== null) console.log(m3[0]); else console.log("no-match");
console.log(/(a?)\1b/.test("b"));
console.log("xyxy".replace(/(?<p>xy)\k<p>/, "<$<p>>"));
// case-insensitive backref
console.log(/^(ab)\1$/i.test("abAB"));
