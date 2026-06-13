// RFC 20260613 any-class-member-read — Member read on Type::Any
// against a class instance (Error subtree, user class) now goes
// through inline class_tag dispatch + direct field load instead of
// the dynobj entry walk (which would not find class fields and
// returned undefined for every site).

// Case A: user-thrown TypeError carries its message + name when
// caught as `any`. Pre-fix: e.name === undefined / e.message ===
// undefined. Post-fix: TypeError / custom-msg-A.
try {
  throw new TypeError("custom-msg-A");
} catch (e: any) {
  console.log("A typeof:", typeof e, "name:", e.name, "message:", e.message);
}

// Case B: any-typed binding to a fresh user class instance reads
// declared fields via class_tag dispatch.
class Foo {
  a: string;
  b: number;
  constructor(a: string, b: number) {
    this.a = a;
    this.b = b;
  }
}
const y: any = new Foo("yo", 7);
console.log("B a:", y.a, "b:", y.b);

// Case C: plain ObjectLit through Any still walks the dynobj entry
// table — no class_tag, falls through to the dynobj_get path.
const o: any = { hello: "world", n: 42 };
console.log("C hello:", o.hello, "n:", o.n);

// Case D: missing field on a class instance through Any returns
// undefined (the class_tag matched but the field name is absent
// from every candidate's layout — the dispatch chain falls through
// to dynobj_get which returns ANY_UNDEF for a non-dynobj ptr).
const z: any = new Foo("x", 0);
console.log("D missing:", z.unknown);

// Case E: typed read still works (separate code path; this test
// guards against accidental SSA-layer regression).
const typed = new Foo("hi", 1);
console.log("E typed:", typed.a, typed.b);
