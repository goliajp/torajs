// A nullable data member read through an interface after a null test
// narrows it: the read still goes where the class laid the member out.
interface Q {
  id: number | null;
  name: string;
}
class Pad {
  extra: boolean;
  id: number | null;
  name: string;
  constructor(id: number | null) {
    this.extra = true;
    this.id = id;
    this.name = "pad";
  }
}
function a(r: Q): string {
  return r.id === null ? "none" : "v" + r.id;
}
function b(r: Q): number {
  if (r.id !== null) return r.id + 1;
  return -1;
}
function c(r: Q): string {
  if (r.id) return r.name + "=" + r.id;
  return r.name + "?";
}
const lit: Q = { id: 8, name: "lit" };
const qs: Q[] = [new Pad(4), new Pad(null), lit];
for (const q of qs) console.log(a(q), b(q), c(q));
