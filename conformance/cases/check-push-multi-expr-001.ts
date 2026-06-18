// S157 — `Array.prototype.push(...items)` multi-arg in expression
// position (LetDecl init / Return) per ES §22.1.3.20. Pre-fix only
// the Stmt::Expr (statement) position desugared multi-arg into
// sequential single-arg calls; expression-position multi-arg push
// fell through to ssa_lower's `args.len() == 1` guard and failed
// with "unsupported member call shape: push". push's spec return =
// FINAL array length, so hoisting (n-1) sibling calls before and
// leaving a single-arg push as the original expression preserves
// the user-observed value.

// 1) `const rv = r.push(a, b, c)` — LetDecl-position multi-arg.
const r: Array<number> = [];
const rv = r.push(1, 2, 3);
console.log(rv);
console.log(r.length, r[0], r[1], r[2]);

// 2) Refcounted element type (Array<string>) via expression-position.
const s: Array<string> = ["x"];
const sv = s.push("y", "z", "w");
console.log(sv);
console.log(s.join(","));

// 3) `return r.push(a, b, c)` — Return-position multi-arg.
function build(): number {
  const a: Array<number> = [];
  return a.push(10, 20, 30, 40);
}
console.log(build());

// 4) Statement-position multi-arg (pre-existing S132 path) — must
//    still work after the desugar refactor.
const t: Array<number> = [];
t.push(1, 2);
console.log(t.length, t[0], t[1]);

// 5) Single-arg push in both stmt and expr position — unchanged.
const u: Array<number> = [];
u.push(99);
const uv = u.push(100);
console.log(uv, u[0], u[1]);

// 6) `let` receiver with expression-position multi-arg push.
let w: Array<number> = [];
const wv = w.push(7, 8, 9);
console.log(wv, w[0], w[1], w[2]);
