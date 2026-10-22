// An arrow handed to a method takes the parameter types the method
// declares for that position, as one handed to a plain function does.
// Which declaration that is follows the receiver's static type: an
// object literal bound once, a declared object type, a class (through
// `this` too). Values must arrive whole through the typed parameters.
// A receiver whose method could come from elsewhere (a spread, a
// subclass that inherits it, a name two functions both bind) assumes
// nothing.
const o = {
  arm(f: (res: (v: number) => void) => void) {
    f((x) => console.log("lit-inner", x + 0.5));
  },
};
o.arm((res) => res(1));

type Box = { each: (cb: (n: number, i: number) => void) => void };
const b: Box = {
  each(cb) {
    cb(2.5, 0);
    cb(3.5, 1);
  },
};
b.each((n, i) => console.log("typed-field", n * 2, i));

class Bag {
  items: number[] = [1.5, 2.5];
  visit(fn: (x: number) => void): void {
    for (const it of this.items) fn(it);
  }
  run(): void {
    this.visit((x) => console.log("this-method", x + 1));
  }
}
const bag = new Bag();
bag.visit((x) => console.log("class-method", x * 2));
bag.run();
const bag2: Bag = new Bag();
bag2.visit((x) => console.log("annotated", x - 1));

class Sub extends Bag {}
const sub = new Sub();
sub.visit((x) => console.log("inherited", x));

const words = { arm(f: (s: string) => void) { f("a"); } };
const nums = { arm(f: (n: number) => void) { f(9.25); } };
words.arm((s) => console.log("words", s + "!", s.length));
nums.arm((n) => console.log("nums", n + 1));

function shadowA(): void {
  const o2 = { go(f: (s: string) => void) { f("in"); } };
  o2.go((s) => console.log("shadow-a", s.length));
}
function shadowB(): void {
  const o2 = { go(f: (n: number) => void) { f(5.5); } };
  o2.go((n) => console.log("shadow-b", n * 2));
}
shadowA();
shadowB();

const spread = {
  ...{ go(f: (n: number) => void) { f(7); } },
  go2(f: (n: number) => void) { f(8); },
};
spread.go2((n) => console.log("spread", n + 0.5));

const multi = {
  pair(f: (a: number, b: string) => string): string {
    return f(1.5, "x");
  },
};
console.log("multi", multi.pair((a, b) => b + a * 2));
