// A relay shell (`__forward_<t>` for `(t as any)(…)`, `__fwdrecv_<t>`
// for `xs.map(t, o)`) is minted as the target's own face, and the
// boxed-entry redirect enters the target directly only when the two
// signatures match slot for slot. The width analysis used to widen a
// target parameter to F64 without its shell twin, so the faces no
// longer matched, the shell body's static forwarding call ran
// instead, and — packing the declared count — `(t as any)(1, 2, 3,
// 4)` answered `arguments.length` 3 (rotation 650). A shell param now
// shares its target param's width.
const o = { k: 1 };
const xs: any = [7];
function t(this: any, a: number, b: number, c: any): any { return [a, b, c === undefined, arguments[1], arguments[2] === undefined, arguments.length, typeof this]; }
console.log("anycall", JSON.stringify((t as any)(1, 2, 3, 4)));
console.log("map", JSON.stringify(xs.map(t, o)));
function h(this: any, a: number, b: number): any { return [a, b * 0.5, arguments.length, typeof this]; }
console.log("anycall-h", JSON.stringify((h as any)(1, 3, 5, 7)));
console.log("map-h", JSON.stringify(xs.map(h, o)));
function g(a: number, b: number): any { return [a, b * 0.5, arguments.length, arguments[3]]; }
console.log("anycall-g", JSON.stringify((g as any)(1, 3, 5, 7)));
console.log("map-g", JSON.stringify(xs.map(g)));
