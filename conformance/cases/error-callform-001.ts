// rotation 140 — Error family called as a function (ES §20.5.1.1:
// the Error constructor performs the same steps when called as a
// function as when new'd). test262 built-ins/Error length.js /
// internal-prototype.js / instance-prototype.js /
// the-initial-value-of-errorprototypemessage: `Error("err")`
// previously fell through to the dynamic any call path and threw
// "value is not a function". Desugar now rewrites the call form to
// Expr::New, riding the same rewrite Array(...) already had.

const e1 = Error("plain");
console.log(e1 instanceof Error, e1.message, e1.name);

const t1 = TypeError("typ");
console.log(t1 instanceof TypeError, t1 instanceof Error, t1.message, t1.name);

const r1 = RangeError("rng");
console.log(r1 instanceof RangeError, r1.message);

const s1 = SyntaxError("syn");
const f1 = ReferenceError("ref");
const v1 = EvalError("evl");
const u1 = URIError("uri");
console.log(s1.name, f1.name, v1.name, u1.name);

// No-arg call form: message stays own-absent.
console.log(Error().hasOwnProperty("message"), Error().message);

// The call form still throws like the construct form.
try {
  throw TypeError("boom");
} catch (err: any) {
  console.log(err instanceof TypeError, err.message);
}

// toString through the call form. (`String(TypeError("y"))` is the
// recorded String(ClassRef) coercion gap — L3b, not this blade.)
console.log(Error("x").toString(), TypeError("y").toString());
