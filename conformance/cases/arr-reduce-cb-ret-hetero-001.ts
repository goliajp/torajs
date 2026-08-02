// reduce/reduceRight callback return polymorphism — a hetero typed
// ret rides the Any-acc lane (§23.1.3.24: the acc is the seed before
// the first call and the cb ret after; an empty walk answers the
// seed untouched).
var called = 0;
function cb0() { called++; return true; }
console.log([11, 12].reduceRight(cb0, 11), called);
console.log([1, 2, 3].reduce(cb0, 0));
console.log([].reduce(cb0, 7));
console.log([5].reduceRight(cb0, 9));
function cbStr(prev, cur) { return "s" + cur; }
console.log([1, 2].reduce(cbStr, "x"));
console.log([1, 2].reduce(cbStr));
console.log([1, 2, 3].reduce(function (a, b) { return a + b; }, 10));
console.log([1.5, 2.5].reduce(function (a, b) { return a + b; }, 0));
