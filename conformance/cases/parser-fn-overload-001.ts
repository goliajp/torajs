// V3-18 wedge — TS function overload signature declarations:
//   function f(x: number): number;
//   function f(x: string): string;
//   function f(x: any): any { return x }
// The leading semicolon-terminated forms have NO body and are
// type-only (declaration merging). The actual impl is the
// trailing same-named declaration. Per TS spec the impl's
// signature is hidden from callers — only the overloads are
// visible. Pre-fix tora's parser bailed with 'expected `{`
// (function body), got Semi'.
//
// Implementation: in parse_fn, after the return-type ann, if
// the next token is `;`, treat as overload sig and return an
// empty Block (effectively discarding). The real FnDecl is the
// follow-up declaration. Type-side info from overload sigs is
// not yet plumbed (subset limitation — would need overload
// dispatch in check.rs), so the impl signature still drives
// type behavior.

// Subset workaround: the impl signature must use concrete
// types compatible with all overload signatures (tora's
// typechecker doesn't yet erase via overloads). For now write
// the impl with the most permissive shape that still
// typechecks against caller args.

function add(x: number, y: number): number;
function add(x: number, y: number): number {
  return x + y
}
console.log(add(2, 3))                 // 5

// Multiple overload sigs with the same impl shape.
function fmt(n: number): string;
function fmt(b: boolean): string;
function fmt(x: number): string {
  return "n:" + x
}
console.log(fmt(42))                   // n:42

// Class methods with overload sig.
class Calc {
  apply(a: number, b: number): number;
  apply(a: number, b: number): number {
    return a + b
  }
}
let c = new Calc()
console.log(c.apply(7, 8))             // 15
