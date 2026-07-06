// RFC 20260705 ledger #3 chunk 571 — coercion/ctor call args share:
// the runtime helpers borrow (or internally share) their arg, so the
// source binding stays live after the call. Pre-571 these sites
// consumed an Ident arg (orphaned stake = leak) and String(s) stole
// the source's stake (UAF after the result's owner dropped).

// 1. Number(s) — str_to_number borrows; s survives.
let s1 = "12" + "34";
let n1 = Number(s1);
console.log(n1, s1);

// 2. String(s) pass-through shares — t drops in its block, s survives.
let s2 = "val" + "42";
{
  let t = String(s2);
  console.log(t);
}
let filler1 = "AAAA" + "BBBB";
console.log(s2, filler1);

// 3. Boolean(s) — s survives.
let s3 = "x" + "y";
let b3 = Boolean(s3);
console.log(b3, s3);

// 4. isNaN / isFinite over string — s survives.
let s4 = "7" + "8";
console.log(isNaN(s4), isFinite(s4), s4);

// 5. Symbol(desc) — symbol_alloc shares the desc; s survives.
let s5 = "desc" + "5";
let sym5 = Symbol(s5);
console.log(sym5.toString(), s5);

// 6. Symbol.for(key) / Symbol.keyFor — key survives.
let k6 = "reg" + "6";
let sym6 = Symbol.for(k6);
console.log(Symbol.keyFor(sym6), k6);

// 7. BigInt(s) — bigint_from_str borrows; s survives.
let s7 = "123" + "456";
let big7 = BigInt(s7);
console.log(big7.toString(), s7);

// 8. BigInt.asIntN(bits, v) — v survives.
let v8 = BigInt("300");
let w8 = BigInt.asIntN(8, v8);
console.log(w8.toString(), v8.toString());

// 9. any-receiver dynamic string key — key survives the probe.
let o9: any = { alpha: 9 };
let k9 = "alp" + "ha";
console.log(o9[k9], k9);

// 10. obj.hasOwnProperty(runtime key) — key survives (572 preview kept
// out; only the coercion-family stations assert here).
// (universal-methods station lands in chunk 572)

// 11. owned-temp args still release (no leak, no crash): concat temps
// straight into the coercers.
console.log(Number("9" + "9"), Boolean("a" + "b"), String("c" + "d"));
console.log(BigInt("11" + "22").toString());
console.log(isNaN("5" + "5"));
