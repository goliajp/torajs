// §23.1.3.19 `Array.prototype.map` answers an array of whatever the
// callback returns. The checker's heterogeneous-return arm used to
// accept four of those -- Number, String, Boolean, Any -- because
// those were once the only element widths `emit_map`'s destination
// could hold. That stopped being true (an owned heap value already
// rode across the push unchanged), but the list stayed, and it was
// the only thing standing between the most ordinary shape in the
// language and an answer: `xs.map(x => ({ ... }))` was a compile-
// time type error quoting the method table's homogeneous signature.

// An object literal per element -- the shape the list was rejecting.
const wrapped = [1, 2, 3].map(x => ({ v: x }));
console.log(wrapped.length, JSON.stringify(wrapped));

// More than one field, and a field read back off the result.
const pairs = [1, 2, 3].map(x => ({ v: x, w: x * 2 }));
console.log(JSON.stringify(pairs), pairs[1].w);

// A object-typed source mapped to a different object shape -- the
// callback's parameter type and its return type differ, which is
// the whole point of the arm.
const rows = [{ id: 1 }, { id: 2 }];
const widened = rows.map(o => ({ id: o.id, twice: o.id * 2 }));
console.log(JSON.stringify(widened));

// A string source, an object result.
const named = ["a", "bb"].map(s => ({ k: s, n: s.length }));
console.log(JSON.stringify(named));

// Arrays are the same story -- `map` does not flatten (that is
// flatMap), so the result is an array of arrays.
console.log(JSON.stringify([1, 2].map(x => [x, x + 1])));
console.log(JSON.stringify([1, 2].map(x => [{ v: x }])));

// The result is an ordinary array: iterate it, and map it again
// back down to a scalar.
let total = 0;
for (const o of wrapped) {
  total = total + o.v;
}
console.log(total, JSON.stringify(wrapped.map(o => o.v)));

// The primitive lanes the list did accept still answer the same.
console.log(JSON.stringify([1, 2, 3].map(x => "n" + x)));
console.log(JSON.stringify([1, 2, 3].map(x => x > 1)));
