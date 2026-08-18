// cluster #6 (rotation 442): an un-narrowed match/exec result
// (Nullable(Array(String))) rides the `+` string-concat lane —
// §13.15.3 ToString covers both arms: null → "null", array → join.
const s = "abcabc";
const re = /b(c)/;
console.log("got: " + s.match(re));
console.log("miss: " + s.match(/zz/));
let m = s.match(re);
console.log("let: " + m);
let n = s.match(/zz/);
console.log("letmiss: " + n);
console.log(s.match(re) + "!");
console.log(re.exec(s) + "?");
console.log(/zz/.exec(s) + "?");
