// V0.2 P14-S1 — fn-scope const RegExp LICM. `/pat/flags`
// literal inside a loop body is hoisted to fn entry block by
// ssa_lower.rs Expr::Regex arm; the SSA `Call regex_compile`
// runs once per fn invocation regardless of loop iter count.
// Bench: str-replace-100k 156 → 121 ms (−22.5%, ~350 ns/iter
// saved). Spec contract: String.prototype.{replace, match}
// reset lastIndex internally so fn-scope sharing of the
// underlying RegExp is unobservable on these methods.
// `String.prototype.search(RegExp)` / `.split(RegExp)` /
// `.replaceAll(RegExp)` typecheck paths still reject RegExp
// arg today (carried separately in L3b — independent of
// LICM; once those widen, this fixture should grow).

function loop_replace(): number {
  let total: number = 0;
  for (let i: number = 0; i < 5; i = i + 1) {
    let s: string = "abbc xxx abc yyy abbbbc";
    let r: string = s.replace(/a(b+)c/g, "XY");
    total = total + r.length;
  }
  return total;
}

function loop_match(): number {
  let n: number = 0;
  for (let i: number = 0; i < 4; i = i + 1) {
    let s: string = "foo123bar456baz789";
    let m: string[] | null = s.match(/[a-z]+/g);
    if (m !== null) {
      n = n + m.length;
    }
  }
  return n;
}

function dedupe_two_literals(): number {
  // Two identical literals in one fn — cache should dedupe to
  // one entry-block alloc (one `__torajs_regex_compile` call
  // emitted into BlockId(0) regardless of how many times the
  // literal appears in the body).
  let s: string = "abc";
  let r1: string = s.replace(/b/g, "X");
  let r2: string = s.replace(/b/g, "Y");
  return r1.length + r2.length;
}

function distinct_literals_kept_distinct(): number {
  // Different `(pattern, flags)` keys must NOT share — each
  // gets its own entry-block alloc.
  let s: string = "abc";
  let r1: string = s.replace(/a/g, "X");
  let r2: string = s.replace(/c/g, "Z");
  return r1.length + r2.length;
}

function flags_distinguish(): number {
  // Same pattern, different flags → distinct cache entries.
  let s: string = "AbCbA";
  let r1: string = s.replace(/b/g, "X");
  let r2: string = s.replace(/b/gi, "Y");
  return r1.length + r2.length;
}

console.log(loop_replace());
console.log(loop_match());
console.log(dedupe_two_literals());
console.log(distinct_literals_kept_distinct());
console.log(flags_distinguish());
