// S275 — Array.from(iter, mapFn, thisArg?, ...trailing) per ES
// §23.1.2.1: spec accepts mapFn + optional thisArg; trailing args past
// thisArg silent-drop. tora's check.rs/ssa_lower had `args.len() == 2`
// only, rejecting `Array.from(arr, cb, thisArg)` with "expected 1
// argument(s), got 3" via the static-sig fall-through.

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

// 2-arg baseline.
console.log(Array.from([1, 2, 3], (x: number) => x * 2));
console.log(Array.from("abc", (c: string) => c + "!"));

// 3-arg (mapFn + thisArg) — pre-S275 rejected.
console.log(Array.from([1, 2, 3], (x: number) => x + 10, step("a")));
console.log(Array.from("xyz", (c: string) => c.toUpperCase(), step("b")));

// 4+ arg (mapFn + thisArg + trailing) — pre-S275 rejected.
console.log(Array.from([4, 5, 6], (x: number) => x * 3, step("c"), step("d")));
console.log(Array.from([7, 8], (x: number) => x - 1, step("e"), step("f"), step("g")));

console.log("calls=" + calls);
