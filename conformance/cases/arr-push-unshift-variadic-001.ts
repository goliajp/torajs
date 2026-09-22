// §13.3.8.1 evaluates every argument before §23.1.3.23 push /
// §23.1.3.35 unshift appends the first one; the receiver is evaluated
// once. Multi-argument calls in expression position and on temporary
// receivers used to be rejected at lowering.
const xs = [0];
xs.push(1, xs.length);
console.log(xs.join());
const ys = [0];
ys.unshift(1, ys.length);
console.log(ys.join());

const log: string[] = [];
function f(s: string): string {
  log.push(s);
  return s;
}
const zs = ["z"];
console.log(zs.unshift(f("a"), f("b")), zs.join(), log.join());
console.log(zs.push(f("c"), f("d") + "!"), zs.join(), log.join());

console.log([1].push(2, 3), [1].unshift(2, 3), [1].unshift());
function mk(): number[] {
  log.push("mk");
  return [1];
}
console.log(mk().push(2, 3, 4), log.join());

const o = { a: [1] };
console.log(o.a.push(2, 3), o.a.unshift(-1, 0), o.a.join());

const as: any[] = [1];
console.log(as.push("b", { k: 1 }), as.unshift(null, 2.5), JSON.stringify(as));

const ss: string[] = ["a"];
for (let i = 0; i < 3; i++) ss.push("x" + i, "y" + i);
console.log(ss.unshift("p" + 1, "q"), ss.join("|"));

const fs = [1.5];
console.log(fs.push(2, 3.25), fs.join());
console.log(xs.unshift(), xs.length);

// A user method named push receives all its arguments in one call.
class Bag {
  items: number[] = [];
  push(a: number, b: number): number {
    this.items.push(a * 10 + b);
    return this.items.length;
  }
}
const bag = new Bag();
bag.push(1, 2);
console.log(bag.items.join());

// The call answers the new length to a typed binding; a nullable
// element type takes both members.
const n: number = [0].push(1, 2);
const us: (number | null)[] = [];
console.log(n, us.push(null, 1), us.unshift(2, null), JSON.stringify(us));
