console.log("m body");
class A { tag(): string { return "a"; } }
export class B extends A { }
export class C extends B {
  constructor(public v: number) { super(); }
  name(): string { return "c" + this.v; }
}
