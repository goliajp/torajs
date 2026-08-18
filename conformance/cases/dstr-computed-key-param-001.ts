// §14.3.3 ComputedPropertyName in param-position object
// destructuring — `function f({ [expr]: binding }) {}` and friends.
const k = "a";

function f({ [k]: v }: any) {
  return v;
}
console.log(f({ a: 2 }));

const g = ({ [k]: v }: any) => v;
console.log(g({ a: 3 }));

// default fires on missing, not on present
function h({ [k]: v = 9 }: any) {
  return v;
}
console.log(h({}));
console.log(h({ a: 4 }));

// composite key + mixed static field
function m({ first, ["x" + "y"]: second }: any) {
  return first + second;
}
console.log(m({ first: 10, xy: 5 }));

// nested pattern behind a computed key
function n({ [k]: { b } }: any) {
  return b;
}
console.log(n({ a: { b: 6 } }));

// the key evaluates per call
let calls = 0;
function key(): string {
  calls++;
  return "a";
}
function p({ [key()]: v }: any) {
  return v;
}
console.log(p({ a: 1 }), p({ a: 2 }), calls);

// class method param
class C {
  m({ [k]: v }: any) {
    return v;
  }
}
console.log(new C().m({ a: 7 }));
