// one face redeclared below shadows the whole inherited pair on the
// subclass prototype; a generic subclass overrides a generic pair
class B {
  _v = 1;
  get v(): number { return this._v; }
  set v(x: number) { this._v = x; }
}
class S extends B {
  set v(x: number) { this._v = x * 2; }
}
class G extends B {
  get v(): number { return this._v + 5; }
}
const s = new S();
const ds = Object.getOwnPropertyDescriptor(S.prototype, "v")!;
console.log(ds.get === undefined, typeof ds.set, "v" in s);
const dg = Object.getOwnPropertyDescriptor(G.prototype, "v")!;
console.log(typeof dg.get, dg.set === undefined, "v" in new G());
console.log(Object.getOwnPropertyNames(S.prototype).join(","));
class Box<T> {
  item: T;
  constructor(item: T) { this.item = item; }
  get value(): T { return this.item; }
  set value(v: T) { this.item = v; }
}
class Wrap<T> extends Box<T> {
  constructor(item: T) { super(item); }
  get value(): T { console.log("wrap get"); return this.item; }
  set value(v: T) { console.log("wrap set"); this.item = v; }
}
function peek(b: Box<number>): number { return b.value; }
function poke(b: Box<number>, v: number): number { b.value = v; return b.item; }
console.log(peek(new Box(1)), peek(new Wrap(2)));
console.log(poke(new Box(1), 3), poke(new Wrap(2), 4));
const wn = new Wrap<number>(6);
console.log(wn.value + 1);
wn.value = 8;
console.log(wn.item);
