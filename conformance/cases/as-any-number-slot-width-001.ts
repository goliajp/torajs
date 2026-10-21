// The width analysis reads `x as number` over an Any source as f64,
// because the lowering materializes that numeric face with a ToNumber
// and a ToNumber answers an f64. The other direction had no such rule:
// `x as any` fell through to the width of what went IN, and what went
// in says nothing about it — the value leaves the typed world there.
//
// So `let n: number = ("2.5" as any)` asked the width of a Str, got
// "not a number source", left the slot at its default I64, and
// truncated the 2.5 the ToNumber had produced. A source whose ToNumber
// is NaN came out as 0, which is worse: NaN and 0 are both numbers,
// and only one of them is wrong quietly.
//
// Since then the slot no longer converts at all: an `as any` cast
// converts nothing and the annotation has no runtime meaning, so the
// binding holds the value itself from its declaration, as bun does.
// The lines below keep that answer for each source.

let frac: number = "2.5" as any;
console.log("str-frac", frac);

let neg: number = "-0.125" as any;
console.log("str-neg", neg);

let nan: number = new Map() as any;
console.log("map-nan", Number.isNaN(nan));

let re: number = /x/ as any;
console.log("regex-nan", Number.isNaN(re));

// An integer source still reads as one.
let int: number = "7" as any;
console.log("str-int", int, Number.isInteger(int));

// A Date's valueOf is its epoch millis, an integer.
let d: number = new Date(5) as any;
console.log("date", d);

// Already-Any sources were never broken and stay put.
const v: any = 2.5;
let a: number = v;
console.log("any-var", a);
