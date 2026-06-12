// Regex no-match results are null — spec §22.2.7.2 step 9.a (exec)
// and §22.2.7.5 (match, both global and non-global). Previously tr
// returned an empty array, which made every existing
// `m === null ? ... : ...` guard silently take the wrong branch.
// console.log(null-result) prints `null` (bun parity).

const re = /z+/;
const m = re.exec("abc");
console.log(m);
console.log(m === null ? "null-yes" : "null-no");

const m2 = "abc".match(/z/);
console.log(m2);

const m3 = "abc".match(/z/g);
console.log(m3);

// Global exec exhaustion: two hits then null + lastIndex reset.
const reg = /b/g;
reg.exec("abcb");
reg.exec("abcb");
console.log(reg.exec("abcb"));
