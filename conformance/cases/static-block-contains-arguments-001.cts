// §15.7.1 states the same early error twice — once for a field
// initializer, once for a static block: "It is a Syntax Error if
// ContainsArguments of ClassStaticBlockStatementList is true". tr had
// the walk, wired to the first of the two productions only.
//
// The refusals cannot appear in a fixture that runs. What can is the
// shape of the recursion, which is the part that is easy to get wrong
// in the other direction: ContainsArguments pierces arrows (they
// inherit the outer binding) and STOPS at anything that binds its own
// `arguments`. Each line below is a place the word is legal.

var log: any[] = [];

class A {
  static {
    // A function expression binds its own — the walk stops here.
    var f = function () {
      return arguments.length;
    };
    log.push(f(1, 2, 3));

    // A property NAME is not an IdentifierReference.
    var o = { arguments: "as-a-key" };
    log.push(o.arguments);

    // A nested class's method body binds its own too.
    class D {
      m() {
        return arguments.length;
      }
    }
    log.push(new D().m(9));
  }
}

console.log(log[0], log[1], log[2], typeof A);

// And outside a class the word is ordinary.
function outside() {
  return arguments.length;
}
console.log(outside(1, 2));
