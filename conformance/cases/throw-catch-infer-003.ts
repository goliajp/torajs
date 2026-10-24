// An unannotated catch takes the type its try body throws only when
// the throw checks the body and its callees lowered to agree.
//
// The try bodies below throw only a Fail, which pass 0.5 reads off the
// AST, and each also reaches an `instanceof` whose right side is no
// function, which raises a TypeError the AST scan does not count. The
// lowering emits a check for that raise with no call behind it, so the
// parameter stays `any`, and `(e as Fail).code` of the TypeError is
// `undefined`. Typed as a Fail, the binding stopped the program.

class Fail {
  code: number
  constructor(code: number) {
    this.code = code
  }
}

function checks(n: number, rhs: any): boolean {
  if (n > 5) throw new Fail(n)
  return n instanceof rhs
}

function viaCallee(n: number): void {
  try {
    console.log(checks(n, 5))
  } catch (e) {
    console.log("callee", (e as Fail).code)
  }
}

function inBody(n: number, rhs: any): void {
  try {
    if (n > 5) throw new Fail(n)
    console.log(n instanceof rhs)
  } catch (e) {
    console.log("body", (e as Fail).code)
  }
}

viaCallee(1)
viaCallee(9)
inBody(1, 5)
inBody(9, 5)
