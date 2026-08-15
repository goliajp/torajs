class Plain {
  id(): string {
    return "plain";
  }
}
class GenSub<T> extends Plain {
  v: T;
  constructor(v: T) {
    super();
    this.v = v;
  }
  id(): string {
    return "gensub " + this.v;
  }
}
function run(): void {
  const g = new GenSub<number>(5);
  console.log(g.id());
  const p: Plain = g;
  console.log(p.id());
  const base = new Plain();
  console.log(base.id());
}
run();
