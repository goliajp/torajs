// Automatic Semicolon Insertion — restricted-production sites per
// ES §12.9.1. Pre-fix the parser ignored `[no LineTerminator here]`
// in two productions:
//
// 1. §14.10 `return [no LT] Expression? ;` — `return\n1;` parsed as
//    `return 1;` (fn body returned 1); spec forces `return;` then `1`
//    as a bare expression statement.
//
// 2. §12.5.8 / §12.5.9 `LHS [no LT] (++|--)` — `x\n++y` bound `x++`
//    as postfix and left `y` as a bare identifier; spec forces
//    `x; ++y` (the `++` opens the next stmt as a prefix operator on
//    `y`).
//
// Fix: `Parser` now carries the source `&str` alongside the token
// stream so a `has_newline_before(tok_pos)` probe can scan the
// between-token slice for `\n` / `\r` / U+2028 / U+2029. The two
// productions consult it — `parse_return` treats a leading LT as
// `return;`, `parse_postfix`'s `++` / `--` arm bails out of the
// postfix loop.
//
// test262 hits (rotation 106 verdict = bug:exit 1):
//   language/asi/S7.9_A3 (return newline)
//   language/asi/S7.9.2_A1_T5 (postfix ++ after newline)
//   language/asi/S7.9_A5.2_T1 (prefix ++)
//   language/asi/S7.9_A5.4_T1 (prefix --)
//   language/asi/S7.9_A5.6_T1 (postfix ++ with LT before op)
//   language/asi/S7.9_A5.6_T2 (postfix -- with LT before op)

// return newline — the classic footgun.
function f0() {
  return
  1
}
console.log(f0())                        // undefined

function f1(): number {
  return 1
}
console.log(f1())                        // 1  (no LT — expr binds)

function f2(): number {
  return (
    1
  )
}
console.log(f2())                        // 1  (paren wraps expr)

// postfix ++ / -- with newline before the op — must NOT bind.
let a: number = 1
let b: number = 2
let c: number = 3
a = b
++c
console.log("post-inc:", a, b, c)        // 2 2 4

let x: number = 0
let y: number = 0
x
++y
console.log("prefix-inc:", x, y)         // 0 1

let p: number = 5
let q: number = 5
p
--q
console.log("prefix-dec:", p, q)         // 5 4

// postfix ++ / -- SAME LINE — must still bind (the fix does not
// over-consume the "no LT" rule).
let m: number = 10
m++
console.log("same-line-postinc:", m)     // 11

let n: number = 10
n--
console.log("same-line-postdec:", n)     // 9
