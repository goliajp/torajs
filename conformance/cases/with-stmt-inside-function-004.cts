// ES §14.11 `with` written INSIDE a function body — RFC 20260814.
//
// Sloppy goal only, so this fixture is `.cts`.
//
// 刀 1 walked statements only. A function EXPRESSION's body is a
// statement list hanging off an arena expression, not a statement
// child, so a `with` written there was never rewritten and its free
// names resolved lexically — silently answering the outer binding
// where the object carries the name.
//
// What each block pins:
//   - the three routes a body can arrive by: a function DECLARATION
//     (already reached), a function EXPRESSION and an arrow (both of
//     which were not);
//   - a `with` nested two function-expressions deep, so the descent is
//     recursive rather than one level;
//   - the object still wins over a parameter of the enclosing function
//     (the parameter's record sits BEHIND the object's), while a `let`
//     in the with body still shadows the object.

var o: any = { x: "object" };
var x = "outer";

function declared(): string {
  with (o) {
    return x;
  }
}

var expressed: any = function (): string {
  with (o) {
    return x;
  }
};

var arrowed: any = (): string => {
  with (o) {
    return x;
  }
};

console.log(declared(), expressed(), arrowed());

// Two function expressions deep.
var deep: any = function (): any {
  return function (): string {
    with (o) {
      return x;
    }
  };
};
console.log(deep()());

// The object sits in front of the enclosing function's parameter, so
// it wins; a `let` in the body still sits in front of the object.
var byParam: any = function (x: string): string {
  with (o) {
    let seen = x;
    return seen;
  }
};
console.log(byParam("param"));

// A name the object does not carry still falls through to the
// enclosing function's own binding.
var falls: any = function (): string {
  var only = "local";
  with (o) {
    return only;
  }
};
console.log(falls());

// The membership test is re-run per reference, inside a function body
// exactly as at top level.
var grow: any = {};
var later = "before";
var grows: any = function (): string {
  var first = "";
  with (grow) {
    first = later;
    grow.later = "after";
    return first + "/" + later;
  }
};
console.log(grows());

console.log(x);
