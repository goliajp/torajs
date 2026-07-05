// RFC 20260705 ledger #2 (chunk 563) — a concrete refcounted value
// boxed into an `any` slot is always OWNED by the slot: borrow-shape
// inits take +1 before the box, so the source binding keeps its own
// stake and stays readable after the any slot drops or rebinds.
function mk(i: number): string {
  return "hello-" + i;
}

// same-scope Ident init boxed twice (share, not transfer) — the old
// consume path double-freed the source's single stake
let s1 = mk(1);
let v1: any = s1;
let w1: any = s1;
console.log(v1);
console.log(w1);
console.log(s1);

// cross-scope Ident init + re-assign — the any slot's drop-old used
// to steal the outer binding's stake
let s2 = mk(2);
{
  let v2: any = s2;
  v2 = 42;
  console.log(v2);
}
console.log(s2);

// inner block close drops the any slot; the outer source survives
let s3 = mk(3);
{
  let v3: any = s3;
  console.log(v3);
}
console.log(s3);

// Member init (struct field borrow)
let o = { name: mk(4) };
let v4: any = o.name;
console.log(v4);
console.log(o.name);

// container Index init (element borrow)
let a = [mk(5), mk(6)];
let v5: any = a[1];
console.log(v5);
console.log(a[1]);

// owned shapes keep transferring: string indexing view + slice temp
let s6 = "abcdefg";
let v6: any = s6[2];
let v7: any = s6.slice(4);
console.log(v6);
console.log(v7);
console.log(s6);
