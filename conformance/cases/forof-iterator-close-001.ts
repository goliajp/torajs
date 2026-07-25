// RFC 20260725-getiterator-getmethod 刀 6 — a `for..of` that stops
// early owes the iterator a `return()` call (ES §7.4.9 IteratorClose,
// reached from §14.7.5.6 step 5's abrupt completion).
//
// `__torajs_any_iter_close` existed, but only the destructuring lane
// ever emitted a call to it — a `for..of` with a `break` walked away
// from the iterator without a word. That is what runs a user
// iterator's cleanup, so it was silently skipped for every source.
//
// The close is owed only on an EARLY stop. An iterator that reported
// done has closed itself, and calling `return()` on it again would be
// a second cleanup the spec never asks for — so the loop records
// which exit it took rather than closing from the shared exit block
// unconditionally.

function iter(limit: number, log: string[]): any {
  let i = 0;
  return {
    next() {
      i = i + 1;
      return { value: i, done: i > limit };
    },
    return() {
      log.push("closed");
      return { value: 0, done: true };
    },
  };
}

// `break` closes.
const l1: string[] = [];
const s1: any = { [Symbol.iterator]() { return iter(99, l1); } };
let seen1 = "";
for (const v of s1) {
  seen1 = seen1 + String(v);
  if ((v as number) >= 3) break;
}
console.log("break:", seen1, l1.join("|"));

// Running to completion does NOT close — the iterator already did.
const l2: string[] = [];
const s2: any = { [Symbol.iterator]() { return iter(3, l2); } };
let seen2 = "";
for (const v of s2) seen2 = seen2 + String(v);
console.log("natural:", seen2, l2.length);

// A `break` on the very first step still closes.
const l3: string[] = [];
const s3: any = { [Symbol.iterator]() { return iter(99, l3); } };
for (const v of s3) break;
console.log("first-step:", l3.join("|"));

// An iterator with no `return` is already closed — §7.4.9 step 4
// ends there instead of throwing.
const s4: any = {
  [Symbol.iterator]() {
    let i = 0;
    return { next() { i = i + 1; return { value: i, done: false }; } };
  },
};
for (const v of s4) break;
console.log("no-return-method: ok");

// A nested loop closes the inner iterator on each pass.
const l5: string[] = [];
const s5: any = { [Symbol.iterator]() { return iter(99, l5); } };
for (let k = 0; k < 3; k = k + 1) {
  for (const v of s5) break;
}
console.log("nested:", l5.length);

// The builtin lanes have no `return` to call, so an early stop out of
// one is unchanged.
const arr: any = [1, 2, 3];
let a = "";
for (const v of arr) {
  a = a + String(v);
  if ((v as number) >= 2) break;
}
const st: any = "abc";
let b = "";
for (const ch of st) {
  b = b + String(ch);
  break;
}
const m: any = new Map<string, number>();
m.set("k", 1);
m.set("j", 2);
let c = 0;
for (const e of m) {
  c = c + 1;
  break;
}
console.log("builtins:", a, b, c);
