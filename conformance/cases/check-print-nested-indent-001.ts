// inspect indent trunk chunk A — nested composites pad by depth
// (bun's uniform model: every container level adds 2 columns, even
// when the enclosing array stays single-line). Covers dynobj-in-obj,
// obj-in-array (fields at 4 / closer at 2), double-nested
// array-element obj (fields at 6 / closer at 4), obj value holding a
// short single-line array, and the any-boxed variants of the same
// shapes. Break/wrap heuristics (len > 10, composite first element,
// 80-column est) are later chunks — every array here stays
// single-line-eligible on purpose.
console.log({ a: { b: { c: 1 } } });
console.log([1, { k: 1 }]);
console.log([1, [2, { k: 1 }]]);
console.log({ a: 1, b: { c: [1, 2] } });
console.log([true, { k: 1 }]);
console.log({ k: 1 });
console.log([[1], [2]]);
console.log(["s", [1]]);
const anyObj: any = { a: { b: 2 } };
console.log(anyObj);
const anyArr: any = [1, { k: 1 }];
console.log(anyArr);
