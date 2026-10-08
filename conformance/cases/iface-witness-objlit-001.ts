// An object literal of the interface's shape sits next to class
// instances behind the same interface type.
interface Shape {
  area(): number;
  label(prefix: string): string;
}
class Sq {
  s: number;
  constructor(s: number) {
    this.s = s;
  }
  area() {
    return this.s * this.s;
  }
  label(prefix: string): string {
    return prefix + "sq" + this.s;
  }
}
class Ci {
  tag: string;
  r: number;
  constructor(r: number) {
    this.tag = "c";
    this.r = r;
  }
  area() {
    return 3.5 * this.r * this.r;
  }
  label(prefix: string): string {
    return prefix + this.tag + this.r;
  }
}
const lit: Shape = {
  area() {
    return 7.25;
  },
  label(p: string) {
    return p + "lit";
  },
};
const all: Shape[] = [new Sq(2), lit, new Ci(2)];
let total = 0;
for (const x of all) {
  total += x.area();
  console.log(x.label("> "), x.area());
}
console.log(total);
all.push({ area: () => 1, label: (p: string) => p + "arrow" });
console.log(all.length, all[3].label("#"), all[3].area());
