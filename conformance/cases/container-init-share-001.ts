// chunk 570 — container-init lanes share (RFC 20260705 ledger #3):
// Array.of elems take the slot's +1 for borrow-shape args; object-literal
// any-init buckets no longer orphan the source's stake.

// 1. Array.of: source outlives the array
let x = "AAAA" + 1;
{
  let arr = Array.of(x);
  console.log(arr[0]);
}
let c1 = "CCCC" + 2;
console.log(x);
console.log(c1);

// 2. Array.of owned temps + multi-elem
let arr2 = Array.of("DD" + 3, "EE" + 4);
console.log(arr2[0]);
console.log(arr2[1]);
console.log(arr2.length);

// 3. object-literal any-init: ident source stays owned
let s3 = "FFFF" + 5;
let o: any = { p: s3 };
o.p = "GGGG" + 6;
let c2 = "HHHH" + 7;
console.log(s3);
console.log(o.p);
console.log(c2);

// 4. object-literal owned-temp + any-boxed field values
let inner: any = "II" + 8;
let o2: any = { a: "JJ" + 9, b: inner, c: 42 };
console.log(o2.a);
console.log(o2.b);
console.log(o2.c);
console.log(inner);
