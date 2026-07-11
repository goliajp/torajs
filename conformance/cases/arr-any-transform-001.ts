// any-method dispatch backfill chunk 2 — Array.prototype.concat /
// fill / copyWithin / splice through an `any` receiver, both shapes:
// FLAG_ARR_ANY (any-typed decl) and typed-behind-any (`as any` cast,
// whose method-call boundary now marks the elem kind — pre-fix the
// kind-aware arms saw UNSET and threw).
//
// concat is variadic (§23.1.3.1 — array args spread, others append);
// fill / copyWithin wrap relative indices and answer the receiver;
// splice removes + variadically inserts, answering the removed slice.
//
// Acceptance: byte-equal with bun.

// concat — typed receiver, spread + scalar + multi-arg
const xs = [1, 2] as any;
console.log(xs.concat([3, 4]));
console.log(xs.concat(5));
console.log(xs.concat([3], 4, [5, 6]));
console.log(xs.concat());
console.log(xs);

// concat — Arr<Any> receiver with mixed elements
const ma: any = [1, "x", true];
console.log(ma.concat([null, "y"], 7));

// fill — value / range / negative wrap / heap elements
const fa = [1, 2, 3, 4, 5] as any;
console.log(fa.fill(0, 1, 3));
console.log(fa.fill(9, -2));
const fs: any = ["a", "b", "c"];
console.log(fs.fill("z", 1));

// copyWithin — forward / negative target / heap elements
const ca = [1, 2, 3, 4, 5] as any;
console.log(ca.copyWithin(0, 3));
console.log(ca.copyWithin(1, -2));
const cs: any = ["a", "b", "c", "d"];
console.log(cs.copyWithin(-2, 0, 2));

// splice — delete / insert / both / argc shapes
const sa = [1, 2, 3, 4, 5] as any;
console.log(sa.splice(1, 2));
console.log(sa);
console.log(sa.splice(-1));
console.log(sa);
const sb = [1, 2, 3, 4, 5] as any;
console.log(sb.splice(1, 2, 9, 8, 7));
console.log(sb);
const sc = [1, 2, 3] as any;
console.log(sc.splice());
console.log(sc.splice(1));
console.log(sc);
const sd: any = ["p", "q", "r"];
console.log(sd.splice(1, 1, "w", "v"));
console.log(sd);

// chaining — the mutators answer the receiver
const ch = [1, 2, 3, 4] as any;
console.log(ch.fill(0, 2).copyWithin(0, 2).reverse());
