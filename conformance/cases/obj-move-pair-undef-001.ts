// A class instance moved over a binding whose old value is the
// undefined sentinel: the retain paired away with the inline release
// must not touch the sentinel, the moved instance keeps one reference
// per owner, and a chain built this way sums the same as one built
// over null. Acceptance: byte-equal with bun.
class Item {
  id: number;
  link: Item | null;
  constructor(id: number, link: Item | null) {
    this.id = id;
    this.link = link;
  }
}
function total(h: Item | null): number {
  let s = 0;
  while (h !== null) {
    s = s + h.id;
    h = h.link;
  }
  return s;
}

function fromUndef(): number {
  let h: Item | undefined;
  h = new Item(1, null);
  h = new Item(2, h);
  return h.id + total(h.link);
}
console.log("from-undef:", fromUndef());

function fromUndefLoop(n: number): number {
  let h: Item | undefined;
  let prev: Item | null = null;
  for (let i = 0; i < n; i = i + 1) {
    h = new Item(i, prev);
    prev = h;
  }
  return h === undefined ? 0 : h.id + total(h.link);
}
console.log("from-undef-loop:", fromUndefLoop(0), fromUndefLoop(1), fromUndefLoop(5));

function pushUndef(): number {
  const out: (Item | undefined)[] = [undefined, undefined];
  const first = new Item(7, null);
  out[0] = first;
  out[1] = new Item(8, first);
  const last = out[1];
  return (last === undefined ? 0 : last.id + total(last.link)) + out.length;
}
console.log("push-undef:", pushUndef());
