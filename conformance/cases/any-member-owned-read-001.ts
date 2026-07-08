// chunk 717 — any-member fallback owned-result unification: every
// consumer face of an owned member read releases exactly one stake
// (leak fix is RSS-level; this fixture locks the VALUE behavior and
// the share-chain integrity across all consumer shapes).
const re: any = /abc/g;
// let-decl consumer
const src = re.source;
console.log(src);
// direct-chain consumer (.length via any_length_get)
console.log(re.source.length);
// assign consumer (drop-old + owned store)
let s: any = "seed";
s = re.source;
console.log(s);
// literal-key index consumer
console.log(re["source"]);
// optional-chain consumer
console.log(re?.source);
// optional-index consumer
console.log(re?.["source"]);
// console direct-print consumer (single + multi arg)
console.log(re.flags);
console.log("flags:", re.flags);
// share chain intact: the regexp still answers after all reads
console.log(re.source, re.flags);

// class-candidate arm (box_to_any inc) — field read through any
class E {
  message: string = "payload";
}
const e: any = new E();
console.log(e.message);
console.log(e.message.length);
// share chain: two reads answer the same value
const m1 = e.message;
const m2 = e.message;
console.log(m1 === m2, m1);

// accessor arm (invoke_getter owned)
const o: any = {};
let hits = 0;
Object.defineProperty(o, "v", {
  get: () => {
    hits++;
    return "getter-str";
  },
});
const g1 = o.v;
console.log(g1, o.v, hits);

// plain dynobj data property (borrow→owned inc face): value + share
const d: any = { x: "plain-value", n: 42 };
const dx = d.x;
console.log(dx, d.x, d.n);

// fn-name registry read (chunk 716 family)
function topfn(a: number, b: number): number {
  return a + b;
}
const t: any = topfn;
const nm = t.name;
console.log(nm, nm.length, t.length);

// reified method cell read (immortal — release must be a no-op)
const str: any = "hello";
const up = str.toUpperCase;
console.log(up.name, up.length, typeof up);
console.log(up.call(str));

// call-argument consumer: owned read passed straight into a call
function takes(v: any): number {
  return v.length;
}
console.log(takes(re.source));

// binop consumer: owned reads on both sides of a concat
console.log(re.source + "-" + re.flags);
