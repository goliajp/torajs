// The legacy-octal gate (annexB §B.1.1 numeric literals, §B.1.2 string
// escapes) judges by byte range: under a sloppy goal a spelling is a
// SyntaxError only where something else made that region strict code.
// A class is such a region — §15.7 makes every part of one strict —
// so `class C { m() { return 010 } }` must be refused while the same
// literal one line above it is the number 8.
//
// The refusal cannot appear in a fixture that runs. What can, and what
// a byte-range answer actually risks, is the range being wrong: a span
// that reaches one token too far condemns a neighbour, and a span that
// stops one token short lets a class member through. So the octal
// spellings here sit immediately before and immediately after a class,
// with nothing between them and its braces.
//
// tr, bun and node all agree on every line here, so this checks
// against bun in the ordinary way.

console.log(010);
console.log("\101");
class Neighbour {
  m() {
    return "member";
  }
}
console.log(010);
console.log("\101");
console.log(new Neighbour().m());

// A class expression in the middle of an expression is the same
// question with no statement boundary to lean on.
var made = (class {
  n() {
    return "expr-member";
  }
});
console.log(010, new made().n());

// A class with a heritage: the span has to start at `class` and cover
// the clause, so an octal on either side of the whole thing is still
// the sloppy value.
console.log(010);
class WithParent extends Object {}
console.log(010, typeof WithParent);

// A class written inside a `${…}` interpolation is parsed from a
// separate slice, so any byte range minted there is measured from the
// wrong origin — a LOW range, which lands near the top of the file.
// Without the drop on the way out, the `010` on line 1 of this fixture
// was refused by a class written down here.
console.log(`${(class InterpolatedClassWithALongName {}).name}`);
console.log(010);
