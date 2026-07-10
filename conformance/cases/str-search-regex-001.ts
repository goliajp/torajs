// chunk 800 — String.prototype.search(RegExp) per ES §22.1.3.19:
// UTF-16 match index or -1; lastIndex saved/restored (never moves);
// sticky anchors at 0.
console.log("hello world wide web".search(/wide/));
console.log("aab".search(/b/g));
const re = /b/g;
re.lastIndex = 5;
console.log("aab".search(re));
console.log(re.lastIndex);
console.log("aab".search(/b/y));
console.log("baa".search(/b/y));
console.log("héllo wörld".search(/ö/));
console.log("𝄞x".search(/x/));
console.log("abc".search(/z/));
console.log("".search(/a/));
console.log("abc".search(/B/i));
console.log("still works".search("works"));
