// RFC 20260704 S6 — drop correctness for heap values released through `any`.
// 1) string[] moved into any: elements + block freed via the kind-aware
//    runtime walker (looped so pool reuse would expose a double-free).
function makeStrings(): any {
  const t: string[] = ["heap-string-alpha-0123456789abcdef", "heap-string-beta-0123456789abcdef"];
  return t;
}
for (let i = 0; i < 3; i++) {
  const a: any = makeStrings();
  console.log(a[0]);
  console.log(a.length);
}

// 2) nested string[][] moved into any: chain-marked kinds walk recursively.
function makeNested(): any {
  const t: string[][] = [["nested-heap-string-gamma-0123456789"], ["nested-heap-string-delta-0123456789"]];
  return t;
}
const n: any = makeNested();
console.log(n.length);
console.log(n[0][0]);

// 3) typed + any dual owners; any drops first (block scope), typed side
//    must still read its elements afterwards (the static elem walk is
//    rc==1-gated, the runtime walk only fires on hit-zero).
const t2: string[] = ["shared-heap-string-epsilon-0123456789"];
{
  const a2: any = t2;
  console.log(a2[0]);
}
console.log(t2[0]);

// 4) Array<Any> literal straight into any — NaN-box slot walker on hit-zero.
const la: any = [1, "mixed-arr-heap-string-zeta-0123456789", 3.5];
console.log(la[1]);

// 5) dynobj behind any — entry walk + block free on hit-zero.
const o: any = { k: "dynobj-value-heap-string-eta-0123456789" };
console.log(o.k);

// 6) ShortStr coercion temp (materialize → str_to_number → temp drop).
const s: any = "42";
console.log(s * 2);

console.log("done");
