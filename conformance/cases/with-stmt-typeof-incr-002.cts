// ES §14.11 `with` — RFC 20260814 刀 3: `typeof` and `++` / `--`.
//
// Both are single-child nodes, so the whole wrapping node is replaced
// and each arm mints its own operand. Nothing is cloned, which is why
// these two keep §9.1.1.2.1 HasBinding evaluated exactly ONCE — the
// single ResolveBinding the spec performs. (A compound assignment
// cannot be done that way; that is 刀 2.)
//
// `typeof` is the shape that must consult the object BEFORE §13.5.3's
// "unresolvable answers `undefined`" rule applies: `nowhere` below has
// no binding anywhere and must still answer "undefined" rather than
// throwing, while `s` must answer the object's type and not that.

var o: any = { n: 5, s: "str" };
var n = 100;
var outerOnly = 1;

with (o) {
  // object / object / outer binding / nothing at all
  console.log(typeof n, typeof s, typeof outerOnly, typeof nowhere);

  // the increment lands on whichever binding won the lookup
  console.log(n++, n);
  console.log(outerOnly++, outerOnly);
  // postfix only: `--n` / `++n` desugar to an Assign in the parser,
  // which is 刀 2 and is refused by name until then.
  console.log(n--, n);
}

// the outer `n` never moved; the object's did
console.log(o.n, n, outerOnly);

// `typeof` of a name the object grows mid-block flips with it, because
// the membership test is re-run per reference.
var grow: any = {};
with (grow) {
  console.log(typeof appears);
  grow.appears = 1;
  console.log(typeof appears);
}
