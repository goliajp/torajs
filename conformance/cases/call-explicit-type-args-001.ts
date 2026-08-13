// `f<T>(x)` — explicit type arguments at a call site. The grammar is
// genuinely ambiguous with `f < T > (x)`, and TS resolves it by trying
// the type-argument list and committing only if a `(` follows. The
// parser does the same and rewinds otherwise, so the comparisons at
// the bottom keep parsing as comparisons.
//
// Nothing downstream needed changing: the spellings land in the same
// side table `new Box<number>()` already used, which generic inference
// reads.
function id<T>(x: T): T {
  return x;
}
console.log(id<number>(42), id(42));
console.log(id<string>("s"), id("s"));
console.log(id<boolean>(true));

function pair<A, B>(a: A, b: B): string {
  return `${a}|${b}`;
}
console.log(pair<number, string>(1, "x"));
console.log(pair(2, "y"));

// A type parameter that appears only in the RESULT is what explicit
// arguments are for — argument-driven inference has nothing to go on.
function make<T>(): number {
  return 1;
}
console.log(make<string>());

// Nested type arguments close with `>>`, which the lexer hands over as
// one token.
function firstOf<T>(xs: T[]): T {
  return xs[0];
}
console.log(firstOf<number>([7, 8]));

// Comparisons must survive untouched.
const a = 3;
const b = 5;
const c = 7;
console.log(a < b, b > c, a < b === true);
console.log(a < b && c > b);
console.log((a < b ? "lt" : "ge") + (c > a ? "!" : "?"));

// `new C<T>()` keeps working — the shape the side table was built for.
class Box<T> {
  v: T;
  constructor(v: T) {
    this.v = v;
  }
}
console.log(new Box<number>(9).v, new Box<string>("z").v);
