// generic base class with an overridden accessor pair
class Box<T> {
  item: T;
  constructor(item: T) { this.item = item; }
  get value(): T { return this.item; }
  set value(v: T) { this.item = v; }
}
class Loud extends Box<string> {
  get value(): string { return this.item.toUpperCase(); }
  set value(v: string) { this.item = v + "!"; }
}
function peek(b: Box<string>): string { return b.value; }
function poke(b: Box<string>, v: string): string { b.value = v; return b.item; }
console.log(peek(new Box("a")), peek(new Loud("b")));
console.log(poke(new Box("a"), "c"), poke(new Loud("b"), "d"));
const nb = new Box<number>(3);
console.log(nb.value + 1);
