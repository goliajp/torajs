// Rotation 363 knife 4 — reduce/reduceRight join the argv-face
// channels: the reducer packs the WHOLE §23.1.3.24 list
// «previousValue, currentValue, currentIndex, O» through the boxed
// variadic dispatch, and the checker's sister route mirrors the
// lane's resolve_acc_ty (callback ret, widened to Any when the
// seed / first element doesn't share it).
console.log([1, 2, 3].reduce(function () { return arguments.length; }, 0));
console.log([1, 2, 3].reduce(function () { return arguments[0] + arguments[1]; }, 10));
console.log([5, 6].reduce(function () { return arguments[2]; }));
console.log([1, 2].reduceRight(function () { return arguments[0] + arguments[1] * 10; }, 0));
const acc = function () { return arguments[0] + arguments[1]; };
console.log([7, 8].reduce(acc, 100));
console.log([2].reduce(function () { return "s" + arguments[1]; }, "a"));
