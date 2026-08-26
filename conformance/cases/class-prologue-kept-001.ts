// r505 (A12) — the other side of class-prologue-elided-001: every
// way a program observes the class object / prototype cells now
// resolves through the registry (`class_get` / `proto_get`), in main
// and in closures alike — the cells are fn-locals of the synthesized
// prologue, not bindings in scope any more. Each read below is a
// guard reader, so the prologue is KEPT and every answer must match.
class Point {
  x = 1;
  y = 2;
  static origin(): Point {
    return new Point();
  }
  sum(): number {
    return this.x + this.y;
  }
}
class Point3 extends Point {
  z = 3;
  sum(): number {
    return this.x + this.y + this.z;
  }
}
class Even {
  static [Symbol.hasInstance](x: any): boolean {
    return typeof x === "number" && x % 2 === 0;
  }
}
console.log(Point.name, Point.length, Point3.name);
const p = new Point3();
console.log(p.sum(), Point.origin().sum());
console.log(Object.getPrototypeOf(p) === Point3.prototype);
console.log(Object.getPrototypeOf(Point3.prototype) === Point.prototype);
console.log((p as any) instanceof Point, (4 as any) instanceof Even, (5 as any) instanceof Even);
const K: any = Point;
console.log(typeof K, K === Point, (p as any).constructor === Point3);
const nameOf = (): string => Point3.name;
console.log(nameOf());
function describe(): string {
  return Point.name + "/" + Point3.prototype.constructor.name;
}
console.log(describe());
console.log(p);
