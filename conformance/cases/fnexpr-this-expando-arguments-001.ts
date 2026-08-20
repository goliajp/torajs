// rotation 460 — the expando store face and the ARGV face have to
// admit the same store positions. `arguments_object`'s kill walk
// shares `boxed_face_store_target` with the fnexpr-this store arm on
// purpose; widening only the fnexpr side made
// `o.f = function () { saved = arguments }` die with `unknown
// identifier \`arguments\`` — the walk saw a store it did not
// recognize and killed the argv face while the closure kept its
// boxed dual entry.
var o = { a: 1 };
var saved: any;
o.f = function () {
  saved = arguments;
  return (this as any).a;
};
console.log(o.f(7, 8));
console.log(saved[0], saved[1], saved.length);

// The computed-key twin — the ToPrimitive spy idiom test262 writes.
var t = { toISOString: 1 };
var spy: any;
t[Symbol.toPrimitive] = function () {
  spy = arguments;
  return 3.14;
};
console.log(String((t as any)[Symbol.toPrimitive].call(t, "number")));
console.log(spy[0], spy.length);
