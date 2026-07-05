// L3b #15 / chunk 560 — a Substr view crossing into `any` shares
// Tag::Str, so the any-world drop dispatch handed it to
// __torajs_str_drop, which freed it by the Str layout: the view's
// u64 len read as a u32 Str length → wrong-size pool push (heap
// corruption on the next pooled alloc) and the parent ref never
// dec'd. str_drop now routes VIEW/INLINE-flagged cells to
// __torajs_substr_drop off the already-loaded flags word.
let s = "abcdefghij";
let c: any = s[3];
console.log(c);
console.log(typeof c);

// churn — every iteration boxes a fresh single-char view into any
// and scope-drops it; the wrong-size pool push corrupted str-pool
// bookkeeping before the fix.
let last: any = "";
for (let i = 0; i < 100000; i++) {
  let v: any = s[i % 10];
  last = v;
}
console.log(last);

// interleave real str allocations with view drops so a poisoned
// pool would surface as corrupted bytes.
let acc = "";
for (let j = 0; j < 50; j++) {
  let v: any = s[j % 10];
  last = v;
  acc = acc + s[j % 10];
}
console.log(acc.length);
console.log(last);
