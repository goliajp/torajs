// A field or method of a value behind `?.`, or of a union of classes,
// is what each arm declares: one declared `any` hands back its value as
// is from a function declared to return a scalar.
type Box = { v: any };
function pick(o: Box | undefined): number {
  return o?.v;
}
const b: Box = { v: "s" };
console.log(pick(b), pick(undefined));
class C {
  v: any = "c";
  m(): any {
    return "cm";
  }
}
class D {
  v: any = true;
  m(): any {
    return false;
  }
}
function field(o: C | D): number {
  return o.v;
}
function method(o: C | D): number {
  return o.m();
}
function maybe(o: C | null): number {
  return o?.v;
}
function maybeCall(o: C | undefined): string {
  return o?.m();
}
function local(flag: boolean): string {
  const o: C | D = flag ? new C() : new D();
  return o.v;
}
console.log(field(new C()), field(new D()), method(new C()), method(new D()));
console.log(maybe(new C()), maybe(null), maybeCall(new C()), maybeCall(undefined));
console.log(local(true), local(false));
