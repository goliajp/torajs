// L3b ④ (scalar shape) — a FnDecl's scalar-typed default that reads
// a PRIOR param must evaluate in the callee's params environment
// (§9.2): the call-site pad pasted the expression into the caller's
// scope (`unknown identifier k` + runtime ReferenceError). The
// TypedNarrow lane moves it into the body behind an undefined guard.
function h(k: number, m: number = k + 1): number {
  return k * 10 + m;
}
console.log(h(3)); // 34
console.log(h(3, 7)); // 37
const u: any = undefined;
console.log(h(3, u)); // 34 (runtime undefined fires the default)

// string lane, prior-ref through a member read
function tag(base: string, full: string = base + "!"): string {
  return full;
}
console.log(tag("hi")); // hi!
console.log(tag("hi", "yo")); // yo

// three-param chain: middle default reads first, last reads middle
function chain(a: number, b: number = a * 2, c: number = b + 1): number {
  return a + b * 10 + c * 100;
}
console.log(chain(1)); // 321
console.log(chain(1, 5)); // 651
console.log(chain(1, 5, 9)); // 951

// non-prior-ref typed default keeps the pad channel (zero movement)
function plain(x: number, y: number = 5): number {
  return x + y;
}
console.log(plain(1)); // 6
console.log(plain(1, 2)); // 3

// class method with a prior-ref default (flattened to __cm_ FnDecl)
class Box {
  base: number;
  constructor(base: number) {
    this.base = base;
  }
  grow(step: number, upto: number = step * 3): number {
    return this.base + step + upto;
  }
}
const bx = new Box(100);
console.log(bx.grow(2)); // 108
console.log(bx.grow(2, 4)); // 106
