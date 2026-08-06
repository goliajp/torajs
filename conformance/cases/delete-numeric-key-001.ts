// ES §13.5.1 `delete o[k]` with a Number key. §7.1.19 ToPropertyKey
// step 3 is ToString for every non-Symbol key, so `o[1]` and `o["1"]`
// are two spellings of one property — including the spellings §7.1.17
// makes surprising (`-0` is "0", `1e21` is "1e+21").

// array element deletion leaves a hole: length is untouched, the index
// stops being an own property, and the read answers undefined.
const arr: any = [1, 2, 3];
console.log(delete arr[1]);
console.log(1 in arr, arr[1], arr.length);
console.log(JSON.stringify(arr));
console.log(Object.keys(arr).join(","));

// every canonical number-to-string spelling reaches its own entry
const o: any = {
  "1": "a",
  "1.5": "b",
  "0": "c",
  "1e+21": "d",
  NaN: "e",
  Infinity: "f",
};
console.log(
  delete o[1],
  delete o[1.5],
  delete o[-0],
  delete o[1e21],
  delete o[NaN],
  delete o[Infinity],
);
console.log(Object.keys(o).length, JSON.stringify(o));

// §13.5.1.2 — deleting an absent key is legal and answers true, so a
// repeated delete answers true both times.
const q: any = { "2": "x" };
console.log(delete q[2], delete q[2], Object.keys(q).length);

// a numeric key computed at run time takes the same route
const k = 3;
const r: any = { "3": "y", "4": "z" };
console.log(delete r[k], Object.keys(r).join(","));
