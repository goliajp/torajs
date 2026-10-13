// A union-typed parameter copied into a `let` or `var` before its member
// or element is read hands back, from a function declared to return a
// scalar, what each arm answers, as is.
function e1(x: string | number): number { let r = x; return r.toString(); }
function e2(x: any[] | string): number { let r = x; return r.at(0); }
function e3(x?: any[] | string): number { let r = x; return r?.[0]; }
function e5(x: any[] | string): number { let r = x; return r.slice(1); }
function e6(x: any[] | string): number { let r = x as any[] | string; return r[0]; }
function e7(x: any[] | string): number { let r = x; return r.length; }
console.log(e1(7) + 1, e2("ab"), e3("cd"), e3(undefined), e5("ef"), e6("gh"), e7("ijk") + 1);

function c1(x: any[] | string): number { let r = x; return r[0]; }
function c9(x: any[] | string): number { let r = x; r = x; return r[0]; }
function d3(x: any[] | string): number { let r = x; return r.length > 9 ? 0 : r[0]; }
function d4(x: any[] | string): number { let r; r = x; return r[0]; }
function d5(x: string | number): number { let r = x; return r.toString(); }
function d6(x: any[] | string): number { var r = x; return r[0]; }
function d7(x: string | number): number { var r = x; return r.toString(); }
function d8(x: any[] | string): number { var r; r = x; return r[0]; }
console.log(c1("ab"), c9("op"), d3("rs"), d4("tu"), d5(8) + 1, d6("vw"), d7(5) + 1, d8("ab"));
console.log(c1([4]) + 1, d6([5]) + 1, e2([6]) + 1, e7([1, 2]) + 1);

// a binding that also takes a value of another type holds either
function m1(x: any[] | string, k: boolean): number {
  let r = x;
  if (k) r = "k";
  return r[0];
}
function m2(x: string | number, k: boolean): number {
  let r = x;
  if (k) r = 3;
  return r.toString();
}
console.log(m1("m", false), m1(["n"], true), m2(4, false) + 1, m2("q", true) + 1);

// a union of classes copied into a `let` answers what each declares
class A { f: any = "a"; }
class B { f: any = 2; }
function k1(x: A | B): number { let r = x; return r.f; }
console.log(k1(new A()), k1(new B()) + 1);

// an element of a receiver whose annotation the walk cannot read (a
// class mixed with a builtin type, a binding that also takes an object)
// may be of any kind
function n1(x: A | string): number { return (x as any)[0]; }
function n2(x: any[] | string, o: { a: number }, b: boolean): number {
  let r: any = x;
  if (b) r = o;
  return r[0];
}
console.log(n1("ab"), n1(new A()), n2("gh", { a: 1 }, false), n2(["i"], { a: 1 }, true));
