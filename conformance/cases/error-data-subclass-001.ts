// rotation 234 — AggregateError (§20.5.7) and SuppressedError
// (§20.5.8): the two standard Error subclasses whose constructors
// carry own data params ahead of the optional message. Injection
// rides the same reference-gated channel as the §20.5.5 four; the
// data params land as own fields.

const errs = [new Error("a"), new Error("b")];
const e = new AggregateError(errs, "multi");
console.log(e.name, e.message);
console.log(e.errors.length, e.errors[0].message);
console.log(e instanceof AggregateError, e instanceof Error);

// message is optional, same §20.5.1.1 face as the whole family.
const bare = new AggregateError([]);
console.log(bare.name, bare.errors.length);

// SuppressedError carries the (error, suppressed) pair.
const s = new SuppressedError(new Error("x"), new Error("y"), "sup");
console.log(s.name, s.message, s.error.message, s.suppressed.message);
console.log(s instanceof SuppressedError, s instanceof Error);

// Thrown and caught with the class face intact.
try {
  throw new AggregateError([1, 2], "boom");
} catch (err) {
  console.log("caught:", err.name, err.message, err.errors.length);
}
