// rotation 560 — a non-ASCII field / method name keeps its
// spelling on the write side too: the own-keys array, the entries
// pairs and the accessor face names are minted
// by decoding the name's WTF-8 bytes into a Str, not by copying them
// in as Latin-1 units (`é` came out as `Ã©`). A struct's keys print
// bare only when they are ASCII identifiers, like a dynobj's.
class P {
  中 = 1;
  㸭 = 2;
  é = 3;
  get 值() {
    return 9;
  }
  set 值(v: number) {}
  方法() {
    return 4;
  }
}
const p: any = new P();
p.x = 5;
p["a-b"] = 6;
p["é2"] = 7;
console.log(p);
console.log(Object.keys(p).join(","));
console.log(Object.getOwnPropertyNames(p).join(","));
console.log(JSON.stringify(Object.entries(p)));
console.log(JSON.stringify(p));
const d = Object.getOwnPropertyDescriptor(P.prototype, "值");
console.log(d?.get?.name, d?.set?.name, p.值, p.方法());
