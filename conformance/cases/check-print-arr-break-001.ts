// inspect wrap trunk chunk B — bun 1.3.14 array break/wrap heuristic
// on the Arr<Any> walker (ConsoleObject.zig:2410-2591):
// - full-break when len > 10 or the FIRST element is composite
// - elements join ", " and wrap after the comma once the width
//   estimate (string quotes uncounted) passes 80 columns
// - close bracket decided independently of the opener
// Typed-array printers (Arr<I64/F64/Bool/Str>) are chunk C — every
// array here is heterogeneous or holds composites so it stays on
// the Any path.
console.log([[1], 2]);
console.log([{ k: 1 }, 2]);
console.log([{ k: 1 }]);
console.log([{}, 2]);
console.log([{ k: 1 }, { j: 2 }]);
console.log([1, [2, { k: 1 }]]);
console.log({ a: [{ k: 1 }] });
console.log([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, { k: 1 }]);
console.log([1, "aaaaaaaaaaaaaaaa", 2, "bbbbbbbbbbbbbbbb", 3, "cccccccccccccccc", 4, "dddddddddddddddd", 5, "eeeeeeeeeeeeeeee", 6, "ffffffffffffffff"]);
console.log([true, 1, "x", null, 2.5, true, 1, "x", null, 2.5, true]);
const anyMix: any = [{ k: 1 }, 2];
console.log(anyMix);
