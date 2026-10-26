// An object literal of an interface whose method returns an any value,
// handed to a function whose unannotated trailing parameter the call
// leaves out (§10.2.1.4 binds it undefined): the generic call path's
// padded branch sees the literal's member stores.
const raw: any = JSON.parse('{"x":1.5}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
function call(o: J, t) {
  return o.m();
}
function plain(o: J) {
  return o.m();
}
function g(): number {
  return 2.5;
}
console.log(call({ m: g, n: 1 }), plain(new S(raw)), g());
