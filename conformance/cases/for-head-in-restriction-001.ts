// §13.13 / §14.7.4 [In] restriction, positive faces — the for-head
// init parses with [~In], but a ternary THEN branch and parentheses
// reset it, and the for-in statement itself is untouched. (The
// negative face — `for (true ? 0 : 0 in {};;)` is a parse-time
// SyntaxError — rides test262 conditional/in-branch-2.)

// then-branch is [+In], bare
for (true ? "x" in { x: 1 } : false; false; );
console.log("then-bare");

// then-branch, parenthesized
for (true ? ("x" in { x: 1 }) : false; false; );
console.log("then-paren");

// the for-in statement is a different production
const o = { k: 1 };
for (const k in o) {
  console.log(k);
}

// ordinary `in` outside any for-head
console.log("a" in o === false);

// §13.10.1 private-name in (moved to its own parse arm this rotation)
class P {
  #x = 1;
  has(v: any) {
    return #x in v;
  }
}
console.log(new P().has(new P()));
