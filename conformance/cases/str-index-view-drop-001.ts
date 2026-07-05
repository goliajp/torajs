// L3b #15 residual / chunk 561 — `s[i]` on a Str receiver emits a
// FRESH standalone Substr view (rc=1), but let-decl's is_alias_init
// treated every Index init as an element borrow: moved=true skipped
// the scope drop (32B leaked per binding; assembly-confirmed — loop
// body had str_char_at and no drop). The assign path's borrow-inc
// had the same misjudgment (+1 never released). Both now classify
// string indexing as owned.
let s = "abcdefghij";
let a = s[0];
let b = s[9];
console.log(a);
console.log(b);
console.log(a + b);

let c = "";
c = s[2];
console.log(c);
c = s[3];
console.log(c);

let last = "";
for (let i = 0; i < 1000; i++) {
  let ch = s[i % 10];
  last = ch;
}
console.log(last);

// substr receiver indexing (substr_slice lane) through both shapes.
let sub = s.slice(2, 8);
let d = sub[1];
console.log(d);
let e = "";
e = sub[2];
console.log(e);
