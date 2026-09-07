// `.length`, `.name` and `.size` each have their own intrinsic, so a
// struct receiver never reaches `member_get`'s struct arm on those
// three keys — and each intrinsic had grown a different subset of it.
// `.length` had the field face and the expando but not the accessor;
// `.name` and `.size` had no struct arm at all.

const f = { length: 5, name: "n", size: 7 };
const wf: any = f;
console.log(wf.length, wf.name, wf.size);

const g = { get length() { return 5; }, get name() { return "n"; }, get size() { return 7; } };
const wg: any = g;
console.log(wg.length, wg.name, wg.size);
// The keyed spelling of the same three reads always answered.
console.log(wg["length"], wg["name"], wg["size"]);
console.log(JSON.stringify(Object.keys(wg)), JSON.stringify(Object.values(wg)));

class C {
  length = 5;
  name = "n";
  size = 7;
}
const c: any = new C();
console.log(c.length, c.name, c.size);

// A class accessor lives on the prototype, and the byte-keyed resolve
// reaches it there.
class D {
  get length() {
    return 5;
  }
  get name() {
    return "n";
  }
  get size() {
    return 7;
  }
}
const d: any = new D();
console.log(d.length, d.name, d.size);

// The getter sees its receiver.
class E {
  a = 3;
  get size() {
    return this.a + 10;
  }
}
const e: any = new E();
console.log(e.size, e["size"]);

// A defineProperty'd expando on a struct cell still answers, and a
// throwing getter still propagates.
const x: any = { q: 1 };
Object.defineProperty(x, "length", { value: 9, configurable: true });
console.log(x.length);
const t = { get size() { throw new TypeError("boom"); } };
const wt: any = t;
try {
  console.log(wt.size);
} catch (err) {
  console.log("threw", err instanceof TypeError);
}

// The shapes that already answered, unchanged.
console.log([1, 2, 3].length, "abc".length, new Map<string, number>().size);
function nf(a: number, b: number) {
  return a + b;
}
console.log(nf.length, nf.name);
const dy: any = {};
Object.defineProperty(dy, "size", { get: function () { return 7; } });
console.log(dy.size);
