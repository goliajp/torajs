// ToString of an array whose elements are themselves heap values.
// `arr_join` is the Array<Str> kernel; it used to be the table's
// default, so a nested array's slot was read as a `*Str` and reported
// length 0 — `String([[1],[2]])` printed nothing at all. Every element
// type without a kernel now declines onto the any lane, which runs the
// real per-element ToString.
type O = { x: number };

const nested: number[][] = [[1], [2, 3]];
console.log(String(nested));
console.log("x" + nested);
console.log(`${nested}`);
console.log(Number(nested));

const deep: number[][][] = [[[1, 2]], [[3]]];
console.log(String(deep));

const strs: string[][] = [["a"], ["b", "c"]];
console.log(String(strs));

const objs: O[] = [{ x: 1 }, { x: 2 }];
console.log(String(objs));
console.log("o=" + objs);

const empty: number[][] = [];
console.log(String(empty), empty.length);

// the typed kernels still answer their own element types
console.log(String([1, 2, 3]), String(["a", "b"]), String([true, false]));
const parts = "p q r".split(" ");
console.log(String(parts), parts.join("-"));
