// P7.1 — built-in `SyntaxError` subclass (spec §20.5.5). Same
// injection path as TypeError; see check-typeerror-001 for the
// machinery rationale.
//
// Acceptance:
//   - new SyntaxError("msg").message / .name
//   - instanceof SyntaxError AND instanceof Error
//   - user subclass `extends SyntaxError` keeps chain + name
//   - throw + typed catch by SyntaxError and by Error parent

// 1. Direct instantiation.
const e1 = new SyntaxError("unexpected token");
console.log(e1.message);               // unexpected token
console.log(e1.name);                  // SyntaxError
console.log(e1 instanceof SyntaxError); // true
console.log(e1 instanceof Error);      // true

// 2. User subclass of the builtin.
class ParseError extends SyntaxError {
  line: number;
  constructor(msg: string, line: number) {
    super(msg);
    this.line = line;
  }
}
const e2 = new ParseError("missing )", 12);
console.log(e2.message);                // missing )
console.log(e2.name);                   // SyntaxError (inherited)
console.log(e2.line);                   // 12
console.log(e2 instanceof ParseError);  // true
console.log(e2 instanceof SyntaxError); // true
console.log(e2 instanceof Error);       // true

// 3. Throw + typed catch by the builtin subclass.
try {
  throw new SyntaxError("invalid JSON");
} catch (err: SyntaxError) {
  console.log(err.message);             // invalid JSON
  console.log(err.name);               // SyntaxError
}

// 4. Throw + catch by the Error parent.
try {
  throw new ParseError("eof", 3);
} catch (err: Error) {
  console.log(err.message);             // eof
}
