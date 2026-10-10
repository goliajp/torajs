// A spread argument list shorter than a constructor's declared
// parameters: §10.2.11 binds the unreached parameters to undefined.
// No argument was passed there, so the parameter's annotation checks
// nothing.
class Pt {
  x = 5;
}
class C {
  s: string;
  constructor(a: number, p: Pt, q: Pt[]) {
    this.s = arguments.length + ":" + (p === undefined) + ":" + (q === undefined) + ":" + typeof p;
  }
}
const one: any[] = [1];
console.log(new C(...one).s);
const two: any[] = [1, new Pt()];
console.log(new C(...two).s);
class D extends C {
  constructor(...xs: any[]) {
    super(...xs);
  }
}
console.log(new D(2).s);
