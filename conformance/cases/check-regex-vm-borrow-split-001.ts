// P14-S10 — Pike VM `vm_match_at` borrow-split. Eliminates the
// per-iter 528-byte Thread clone on Op::Char / Op::AnyChar / Op::Class
// hot paths by passing `saves` by reference from `ws.cur.list[ti]`
// directly to `add_thread(&mut ws.nxt, ...)` — NLL field-level disjoint
// borrows accept `ws.cur` shared + `ws.nxt` exclusive.
//
// Correctness verification across all five inner-loop ops + capture
// state propagation:
//
// 1. Op::Char + capture groups (the bench shape) — exercises the
//    primary Char-path now passing saves by ref.
// 2. Op::Class — character-class path with multi-byte advance.
// 3. Op::AnyChar — dot matches with /s flag.
// 4. Op::Backref — must still produce correct backref match because
//    handle_backref takes the full owned Thread (cur-aliasing arm).
// 5. End-of-input MATCH — saves writeback from the trailing-thread
//    scan loop after the main while-loop exits.

// (1) /a(b+)c/g — bench shape; capture-group propagation under Char
const re1 = /a(b+)c/g;
const out1 = "abbc xxx abc yyy abbbbc".replace(re1, "XY");
console.log(out1);

// match-all with capture group, verifying saves cleanly carry through
const all1 = "abbc abc abbbbc".match(/a(b+)c/g);
if (all1) {
  console.log(all1.length);
  console.log(all1[0]);
  console.log(all1[2]);
}

// (2) Class — \d+ with capture
const out2 = "x12y345z".replace(/(\d+)/g, "[$1]");
console.log(out2);

// (3) AnyChar — dot with /s flag captures newline
const m3 = "foo\nbar".match(/foo(.)bar/s);
if (m3) {
  console.log(m3[1] === "\n");
}

// (4) Backref — backref to non-empty capture
const m4 = "aabbb".match(/(a+)\1/);
if (m4) {
  console.log(m4[0]);
  console.log(m4[1]);
}

// (5) End-of-input MATCH — trailing thread MATCH after main while exits
const m5 = "abc".match(/abc/);
if (m5) {
  console.log(m5[0]);
}

// (6) Mixed — Char + Class + Match, no capture groups
const c6 = "Hello, World!".replace(/[A-Z]/g, "*");
console.log(c6);
