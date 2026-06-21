// S299 — Array.{at,slice,join}(useful, ...trailing) trailing-arg ignore per
// ES §23.1.3.{1,28,16}. Spec reserves slots past the useful args; tora's
// helpers are 1-/2-/1-arg only. Trailing operands must be evaluated for
// side effects then dropped — step()-counter probes both bun and tora call
// the side-effect expr per ES eval-then-discard semantics.

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

const xs = [10, 20, 30, 40, 50];

// Array.at(idx, ...trailing) — sig 1-arg, drop args[1..].
calls = 0;
const at1 = xs.at(2, step("at-t1") as any, step("at-t2") as any);
console.log("at calls=", calls, "v=", at1);

// Array.slice(start, end, ...trailing) — sig 2-arg, drop args[2..].
calls = 0;
const sl1 = xs.slice(1, 4, step("sl-t1") as any, step("sl-t2") as any);
console.log("slice calls=", calls, "len=", sl1.length, "first=", sl1[0], "last=", sl1[sl1.length - 1]);

// Array.join(sep, ...trailing) — sig 1-arg, drop args[1..].
calls = 0;
const j1 = xs.join("-", step("j-t1") as any, step("j-t2") as any);
console.log("join calls=", calls, "v=", j1);

// at negative idx (spec wrap)
calls = 0;
const at2 = xs.at(-1, step("at-n1") as any);
console.log("at neg calls=", calls, "v=", at2);

// slice with undef end + trailing
calls = 0;
const sl2 = xs.slice(2, undefined, step("sl-u1") as any, step("sl-u2") as any, step("sl-u3") as any);
console.log("slice undef-end calls=", calls, "len=", sl2.length, "first=", sl2[0]);

// join default sep + trailing (only trailing, no sep)
calls = 0;
const j2 = ["a", "b", "c"].join(undefined, step("j-u1") as any, step("j-u2") as any);
console.log("join undef-sep calls=", calls, "v=", j2);
