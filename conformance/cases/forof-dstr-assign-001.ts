// S2.24 (RFC 20260727-dstr-assignment 刀 2) — for-of with a bare
// assignment-pattern head: the pattern writes EXISTING bindings per
// iteration (no declaration), reusing the statement-form expansion.

// array pattern over tuple rows
let a = 0;
let b = 0;
for ([a, b] of [[1, 2], [3, 4]]) {
  console.log(a, b); // 1 2 / 3 4
}
console.log(a, b); // 3 4 — bindings persist past the loop

// object pattern (shorthand + rename)
let x = 0;
let named = 0;
for ({ x, n: named } of [{ x: 5, n: 6 }, { x: 7, n: 8 }]) {
  console.log(x, named); // 5 6 / 7 8
}

// member target inside the pattern
let o = { v: 0 };
for ([o.v] of [[9], [10]]) {
  console.log(o.v); // 9 / 10
}

// C-style for whose init opens with `[` still parses as C-style
let arr = [0, 1, 2];
for ([arr[0]][0]; arr[0] < 2; arr[0]++) {
  console.log("c-style", arr[0]); // c-style 0 / c-style 1
}
