// `String(arr)` and `arr.join()` are the same operation through two
// doors, and the checker admitted only four element types at the
// second one — that list was the `arr_join_*` kernel table, not a
// rule of §23.1.3.18. So `String([[1],[2]])` compiled while
// `[[1],[2]].join(",")` was a type error on the next line.
//
// Each line below is also correct on its own: a program that joins
// nothing else still reaches the element walk, which is what makes
// this a guard rather than a fixture that passes because some other
// statement kept the dispatch families alive.
type O = { x: number };

const nested: number[][] = [[1], [2, 3]];
console.log(nested.toString());
console.log(nested.join());
console.log(nested.join("-"));
console.log(nested.join(undefined));
console.log(nested.toLocaleString());

const objs: O[] = [{ x: 1 }, { x: 2 }];
console.log(objs.join("|"));
console.log(objs.toString());

const strs: string[][] = [["a"], ["b", "c"]];
console.log(strs.join(";"));

const deep: number[][][] = [[[1, 2]], [[3]]];
console.log(deep.join("+"));

// the typed kernels still answer their own element types
console.log([1, 2].join("-"), ["a", "b"].toString(), [true, false].join());
const parts = "p q r".split(" ");
console.log(parts.join("."), parts.toString());
