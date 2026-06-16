// S132 narrow — typed Array<T>.keys() returns ArrIter of 0..length-1.
// runtime `__torajs_arr_iter_create_keys` returns the cursor index
// (`ANY_I64`, i) regardless of slot encoding, so it works uniformly
// over typed Array<T>'s 8B-per-slot raw layout. .values() / .entries()
// still need a box-the-slot walker (independent follow-up trunk).

// Array<number>
const ns: number[] = [10, 20, 30];
for (const k of ns.keys()) {
  console.log("n", k);
}

// Array<string>
const ss: string[] = ["a", "b", "c", "d"];
for (const k of ss.keys()) {
  console.log("s", k);
}

// Array<boolean>
const bs: boolean[] = [true, false, true];
for (const k of bs.keys()) {
  console.log("b", k);
}

// empty array
const es: number[] = [];
let count = 0;
for (const k of es.keys()) {
  count++;
  console.log("e", k);
}
console.log("count", count);
