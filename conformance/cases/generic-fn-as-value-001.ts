// 563-08 — a generic function is a value under its own name.
//
// A generic decl has no lowered original: `monomorphize_and_check`
// emits one instance per call site and nothing under the written
// name. Most positions already reach the function object through
// its forwarder cell, but two asked the question themselves and got
// "no such binding" — a member call with a TYPED ident receiver
// (`console.log(idf)`, and `console` is exactly that) kept its
// arguments unwrapped so that `xs.map(topFn)` stays on raw-FnSig
// direct dispatch, which is a dispatch a generic has no form for;
// and `typeof` read absence-from-`fn_table` as never-declared and
// answered "undefined" for a function that is right there.
function idf<T>(x: T): T { return x }
class SG { static st<T>(x: T): T { return x } }
class SM { m<T>(x: T): T { return x } }

console.log(idf);
console.log(idf, 1);
console.log(typeof idf, typeof SG.st, typeof idf.call);
console.log(SG.st, new SM().m);
console.log(idf.name, idf.length);
console.log([idf], { f: idf });
console.log(idf === idf, idf(1), idf("s"));

const b = idf;
console.log(b, b(2));
function take(f: any) { return f }
console.log(take(idf));
function give() { return idf }
console.log(give());
