// inspect wrap trunk chunk C — bun break/wrap heuristic on the
// typed-array printer family (Arr<I64/F64/Bool/Str>): full-break at
// len > 10, content-line wrap past the 80-column estimate, the
// first-line-inline shape when a single-line opener overflows
// mid-loop, and the same walker reached through nesting (elem-kind
// dispatch) and object values.
console.log([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
console.log([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
console.log([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]);
console.log([1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5, 8.5, 9.5, 10.5, 11.5, 12.5, 13.5, 14.5, 15.5, 16.5, 17.5]);
console.log([true, false, true, false, true, false, true, false, true, false, true, false, true, false, true, false, true]);
console.log(["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc", "dddddddddd", "eeeeeeeeee", "ffffffffff", "gggggggggg"]);
console.log(["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc", "dddddddddd", "eeeeeeeeee", "ffffffffff", "gggggggggg", "hhhhhhhhhh", "iiiiiiiiii", "jjjjjjjjjj", "kkkkkkkkkk", "llllllllll", "mmmmmmmmmm", "nnnnnnnnnn", "oooooooooo", "pppppppppp", "qqqqqqqqqq"]);
console.log([1000000000000000, 2000000000000000, 3000000000000000, 4000000000000000, 5000000000000000, 6000000000000000, 7000000000000000]);
console.log({ xs: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25] });
console.log([[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]]);
