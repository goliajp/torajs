// /gy/ in String.match anchors each successive match (sticky wins
// over global's free search); global match always leaves lastIndex 0
const s: string = "ab ab ab";
console.log(s.match(/ab/gy));
console.log(s.match(/ab/g));
const t: string = "ababab x abab";
console.log(t.match(/ab/gy));
console.log(t.match(/ab/g));
const u: string = " ab";
console.log(u.match(/ab/gy));
console.log(u.match(/ab/g));
const v: string = "aaa";
console.log(v.match(/a*/gy));
// pre-set lastIndex does not survive a global match
const g = /ab/g;
g.lastIndex = 7;
console.log(s.match(g), g.lastIndex);
