// rotation 234 — AggregateError / SuppressedError called as
// functions construct (§20.5.7.1.1 / §20.5.8.1.1 step 1: an
// undefined NewTarget falls back to the active function object),
// the same rule the §20.5.5 family already rides in
// rewrite_error_call.

const e = AggregateError([new Error("a")], "called");
console.log(e.name, e.message, e.errors.length);
console.log(e instanceof AggregateError);

const s = SuppressedError(new Error("x"), new Error("y"), "sup");
console.log(s.name, s.message, s.error.message, s.suppressed.message);
console.log(s instanceof SuppressedError);
