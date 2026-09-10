// §13.15.3: ToNumeric runs on the left operand before the right one, so a left ToPrimitive
// that throws ends the expression without consulting the right operand's hooks — for `*`,
// `&` and `+` alike (test262 expressions/*/order-of-evaluation).
var trace = "";
function MyError() {}
function left() { trace += "1"; return { valueOf: function () { trace += "3"; throw new MyError(); } }; }
function right() { trace += "2"; return { valueOf: function () { trace += "4"; throw new Error("should not be evaluated"); } }; }
for (const op of ["*", "&", "+", "-", "<<", "**", "%"]) {
  trace = "";
  try {
    if (op === "*") (left() as any) * (right() as any);
    else if (op === "&") (left() as any) & (right() as any);
    else if (op === "+") (left() as any) + (right() as any);
    else if (op === "-") (left() as any) - (right() as any);
    else if (op === "<<") (left() as any) << (right() as any);
    else if (op === "**") (left() as any) ** (right() as any);
    else (left() as any) % (right() as any);
    console.log(op, "no throw");
  } catch (e) {
    console.log(op, e instanceof MyError, trace);
  }
}
