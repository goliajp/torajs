// A function's `length` can move — `defineProperty` and `delete` both
// change it (§10.1.6.3 / §20.2.3). The typed lane folded it to the
// declared arity, so one cell answered two different things depending
// on which spelling reached it. `.name` came off its static
// approximation for this reason; this is its twin.

const anon = function (x: number, y: number) {
  return x + y;
};
console.log(anon.length, (anon as any).length);
Object.defineProperty(anon as any, "length", { value: 9, configurable: true });
console.log(anon.length, (anon as any).length);

const arrow = (x: number) => x;
console.log(arrow.length, (arrow as any).length);
Object.defineProperty(arrow as any, "length", { value: 4, configurable: true });
console.log(arrow.length, (arrow as any).length);

// An arrow and an anonymous closure never register an arity row, so
// the compile-time count is what the runtime would have answered —
// it stays as the fallback, not as the answer.
const zero = () => 0;
const three = (a: number, b: number, c: number) => a;
console.log(zero.length, three.length);

class K {
  m(a: number, b: number, c: number) {
    return a;
  }
}
const inst = new K();
console.log(inst.m.length);

// The builtin surfaces, unchanged.
console.log(Math.max.length, [].map.length, "".slice.length);
console.log([1, 2, 3].length, "abcd".length, new Map<string, number>().size);
