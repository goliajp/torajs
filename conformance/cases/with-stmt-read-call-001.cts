// ES §14.11 `with` — RFC 20260814 刀 1 (reads and bare-name calls).
//
// Sloppy goal only, so this fixture is `.cts`: a `with` in a module is
// a SyntaxError and must stay one.
//
// What each block is pinning:
//   - a name the object carries wins over the outer binding, and the
//     outer one is untouched after the block;
//   - a name the object does NOT carry falls through to the outer
//     binding — including one that has no outer binding at all, which
//     is where the guard's else-arm has to stay a plain read;
//   - the membership test is re-run per reference, so a property added
//     between two mentions of the same name changes the second answer;
//   - a bare-name call found on the object is called WITH the object as
//     its receiver (§9.1.1.2.3 WithBaseObject), and one that is not
//     keeps the ordinary undefined-this call;
//   - `let` inside the body shadows the object (its record sits in
//     front), while the object still shadows an outer `var`.

var o: any = {
  x: 1,
  marker: 1,
  who: function (this: any) {
    return this && this.marker === 1 ? "object-receiver" : "other-receiver";
  },
};
var x = 99;
var outerOnly = "outer";

function who(): string {
  return "lexical";
}

with (o) {
  // object wins
  console.log(x);
  // object does not carry it -> outer binding
  console.log(outerOnly);
  // call found on the object: receiver is the object
  console.log(who());
  // a lexical `let` in the body shadows the object
  let x2 = "shadow";
  console.log(x2);
}

console.log(x, outerOnly, who());

// Re-evaluated per reference: the same name answers differently once
// the object grows the property mid-block.
var grow: any = {};
var later = "before";
with (grow) {
  console.log(later);
  grow.later = "after";
  console.log(later);
}

// §14.11.2 step 2 is ToObject, so a null head is a TypeError.
try {
  with (null as any) {
    console.log("unreachable");
  }
} catch (e: any) {
  console.log(e instanceof TypeError);
}

// Nested `with`: the inner guard's fall-through is what the outer one
// rewrites, so the two conditionals ARE the scope chain.
var inner: any = { b: "inner-b" };
var outer: any = { a: "outer-a", b: "outer-b" };
with (outer) {
  with (inner) {
    console.log(a, b);
  }
}
