// RFC 20260815 knife 1 — `extends Base<T>` heritage type arguments are
// consumed and discarded (TS §3.7: no runtime effect). Previously a
// parse error ("expected `{` to begin class body, got Lt").
class Base {
  m() {
    return "base";
  }
  s: string = "field";
}
class K extends Base<any> {}
class L extends Base<{ a: number }> {}
const k = new K();
const l = new L();
console.log(k.m(), l.m(), k.s, l instanceof Base, k instanceof K);
