// An optional method: a class or literal may leave it out. An optional
// call skips the absent ones, a truthiness test reads its presence.
interface Plugin {
  id: string;
  init?(): void;
  run(x: number): number;
}
class Doubler {
  id: string;
  constructor() {
    this.id = "dbl";
  }
  init(): void {
    console.log("init " + this.id);
  }
  run(x: number): number {
    return x * 2;
  }
}
class Adder {
  k: number;
  id: string;
  constructor(k: number) {
    this.k = k;
    this.id = "add" + k;
  }
  run(x: number): number {
    return x + this.k;
  }
}
const plain: Plugin = {
  id: "lit",
  run(x: number) {
    return x - 1;
  },
};
const eager: Plugin = {
  id: "lit2",
  init() {
    console.log("init lit2");
  },
  run(x: number) {
    return x * x;
  },
};
const ps: Plugin[] = [new Doubler(), new Adder(3), plain, eager];
let v = 1.5;
for (const p of ps) {
  const r = p.init?.();
  v = p.run(v);
  console.log(p.id, v, r, p.init ? "has init" : "no init");
}
function start(p: Plugin): string {
  p.init?.();
  return p.id;
}
console.log(start(new Adder(1)), start(new Doubler()));
