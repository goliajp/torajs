// r505 — a multi-arg console.log prints scalar / string arguments
// through their own newline-less printers instead of boxing each
// into the any-value inspector (which rooted every inspectable world:
// `console.log(1, 2)` was 98 KB heavier than `console.log(1)`).
// Every shape below must print byte-for-byte what bun prints.
const i = 42;
const neg = -7;
const big = 9007199254740991;
const f = 1.5;
const nz = -0;
const third = 0.1 + 0.2;
const huge = 1e21;
const tiny = 1e-7;
const nan = NaN;
const inf = -Infinity;
const t = true;
const s = "str";
const empty = "";
const uni = "héllo ✓ 日本";
const sub = uni.slice(1, 4);
const tpl = `t${i}`;
let u: number | undefined;
const n = null;
console.log(i, neg, big, f, nz, third);
console.log(huge, tiny, nan, inf, t, false);
console.log(s, empty, uni, sub, tpl, "lit");
console.log("mix", i, f, t, s, sub);
console.log(u, n, "after", 1);
console.log([1, 2], "arr", [true], ["a", "b"], [1.5, 2]);
console.log({ a: 1 }, "obj", i);
console.log(1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
console.log("a" + "b", i + 1, f * 2, !t, s.length, s.toUpperCase());
console.error("err", i, f, t, s);
function tag(x: number): string { return "n" + x; }
console.log(tag(1), tag, i);
console.log(Symbol("q"), i, 2n, "bigint");
