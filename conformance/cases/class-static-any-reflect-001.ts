// 398-05 — a static method read off the class object through the any
// lane answers its reflection surface: the minted cell carries the
// __sm_ adapter's vaddr, and the fn-name registry now bakes a row
// against that adapter (the __cm_ instance-method mirror), so
// .length (§10.2.10) and .name (§10.2.9) resolve.
class S {
  static s(a: any, b: any): any {
    return a + b;
  }
}
console.log((S as any).s.length);
console.log((S as any).s.name);
const f: any = (S as any).s;
console.log(f(1, 2));

// The compile-time-folded spelling keeps its own row.
console.log(S.s.length, S.s.name);

// An INHERITED static resolves along the [[Prototype]] chain to the
// same cell.
class B {
  static make(a: any, b: any, c: any): any {
    return a;
  }
}
class Sub extends B {}
console.log((Sub as any).make.length, (Sub as any).make.name);

// Default / rest params clamp the arity per SetFunctionLength.
class D {
  static d(a: any, b: any = 1, ...rest: any[]): any {
    return a;
  }
}
console.log((D as any).d.length);
