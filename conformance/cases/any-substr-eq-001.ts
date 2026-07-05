// RFC 20260705 ledger #1 / chunk 562 — strict-eq over an any-held
// Substr view was silently false: anyv_strict_eq's Str×Str arm hands
// both cells to __torajs_str_eq by Tag::Str alone, and the owned-Str
// layout read turned the view's parent-ptr/offset fields into
// "payload" bytes. str_eq now resolves each operand view-aware.
let s = "abcdefghij";
let v: any = s[2];
console.log(v === "c");
console.log(v !== "c");
console.log(v === "d");
console.log("c" === v);

// view vs view (both cells through the same resolve).
let u: any = s[2];
console.log(v === u);
let w: any = s[3];
console.log(v === w);

// substr-receiver indexing lane (substr_slice views).
let sub = s.slice(2, 8);
let x: any = sub[0];
console.log(x === "c");
console.log(x === v);

// loop churn over the eq lane keeps counting correctly.
let hits = 0;
for (let i = 0; i < 10; i++) {
  let ch: any = s[i];
  if (ch === "c") { hits = hits + 1; }
}
console.log(hits);
