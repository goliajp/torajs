// RFC 20260719-fn-tostring-source B6c — a reified class-method face
// (C.prototype.m through the any lane) answers the type-erased
// method-shorthand source through its carried adapter's registry
// row; name, length, and the inspect print resolve the same row.
// Inherited methods answer the declaring class's source.
class C {
  m(a: number): number {
    return a + 1;
  }
  multi(x: number, y: number): number {
    return x * y;
  }
}
class D extends C {
  own(): number {
    return 7;
  }
}
const proto: any = C.prototype;
console.log(proto.m.toString());
console.log(String(proto.m));
console.log(proto.m.name);
console.log(proto.m.length);
console.log(proto.multi.toString());
console.log(proto.multi.length);
console.log(proto.m);
const dproto: any = D.prototype;
console.log(dproto.own.toString());
console.log(proto.m.toString() === String(proto.m));
