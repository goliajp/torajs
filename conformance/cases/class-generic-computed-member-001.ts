// 563-04 — a computed member of a GENERIC class. Two independent
// stops, both fixed here.
//
// (1) It never landed at all. Inside a generic class no method body
// lowers under its own name — only its all-`any` mono instance
// (`$$anywv`) does — so the class-decl-position reify looked up an
// adapter that was not there and skipped the define. `J.prototype`
// answered ["constructor","m","n"], and `j[k]` was undefined.
//
// (2) The row order was mono-emit order, not class-body order. The
// declaration position of a generic class's member is carried by its
// original FnDecl, which does not lower — so it used to be dropped,
// and a computed member's mono (emitted first) put every computed
// member ahead of every plain one.
const k1 = "c1";
const k2 = "c2";

class J<T> { m(x: T) { return x } [k1]() { return 7 } n() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(J.prototype)));
const j: any = new J<number>();
console.log(j[k1](), j.m(3));

// Computed first, plain after.
class P<T> { [k1]() { return 1 } m(x: T) { return x } n() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(P.prototype)));

// Two computed members with plain ones between and after.
class Q<T> { a(x: T) { return x } [k1]() {} b() {} [k2]() {} c() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(Q.prototype)));

// Computed last.
class R<T> { a(x: T) { return x } b() {} [k1]() { return 5 } }
console.log(JSON.stringify(Object.getOwnPropertyNames(R.prototype)));
console.log((new R<string>() as any)[k1]());

// A computed accessor on a generic class.
class G<T> { get [k1]() { return 9 } p(x: T) { return x } }
console.log(JSON.stringify(Object.getOwnPropertyNames(G.prototype)));
console.log((new G<number>() as any)[k1]);

// A computed STATIC member of a generic class (the class object side
// filters the reflection triple — 562-01, see
// class-static-element-order-001).
class S<T> { static m(x: T) { return x } static [k1]() { return 8 } }
console.log(
  JSON.stringify(
    Object.getOwnPropertyNames(S).filter(
      (n: string) => ["length", "name", "prototype"].indexOf(n) < 0,
    ),
  ),
);
console.log((S as any)[k1]());

// A generic method on a generic class keeps its position too.
class H<T> { a(x: T) { return x } pair<U>(v: U) { return v } [k1]() {} b() {} }
console.log(JSON.stringify(Object.getOwnPropertyNames(H.prototype)));

// The printed face reads the same rows. The member's own function
// name is NOT printed here: tr answers `__ccm_<n>__` for
// `w[k1].name` where §15.4 SetFunctionName says "c1" (bun's inspect
// prints `[Function]` for it), so the row would read
// `c1: [Function: __ccm_0__]`. That is 564-01, the same family as
// 562-09 — the implementation's spelling reaching a face the user
// sees — and it is not specific to generic classes.
class W<T> { u(x: T) { return x } [k1]() {} v() {} }
const w: any = new W<number>();
console.log(JSON.stringify(Object.getOwnPropertyNames(W.prototype)), typeof w[k1]);
