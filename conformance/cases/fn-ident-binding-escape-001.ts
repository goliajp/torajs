// A top-level fn name bound to a variable and then used as a value:
// the binding stays a direct call when nothing escapes, and relays
// through its forwarder the moment the local reaches a boxing point.
function add(a: number, b: number): number {
  return a + b;
}
function takeAny(x: any): number {
  return x(3, 4);
}

const direct = add;
console.log(direct(1, 2));

const escaped = add;
console.log(takeAny(escaped));
console.log(escaped(5, 6));
console.log(typeof escaped);

function noArgs(): string {
  return "ok";
}
let alias = noArgs;
const bag: any[] = [];
bag.push(alias);
console.log(bag.length, alias());
