// Chain variant of the name-collision demotion: an inheritance chain
// owning `get` makes desugar rewrite every `x.get(...)` into the
// virtual `__dispatch_get(x, ...)` — a Map receiver must demote back
// to the builtin Map.get path while the chain's own virtual dispatch
// keeps working.
class BaseBox {
  b: number;
  constructor(b: number) {
    this.b = b;
  }
  get(): number {
    return this.b;
  }
}
class SubBox extends BaseBox {
  constructor(b: number) {
    super(b);
  }
  get(): number {
    return this.b * 10;
  }
}
let bb: BaseBox = new BaseBox(3);
let sb: BaseBox = new SubBox(4);
console.log(bb.get());
console.log(sb.get());

let m = new Map<string, number>();
m.set("k", 1.5);
console.log(m.get("k"));
let f: number = m.get("k") as number;
console.log(f * 2);
