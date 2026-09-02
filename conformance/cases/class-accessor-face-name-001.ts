// 566-02 — the class twin of 566-01. A reified accessor face carries
// its §10.2.9 `"get <p>"` name, so `.name` was right; INSPECT was
// not. bun prints `[Function: cg]` — the name in the SOURCE — while
// tr printed `[Function]` for an instance accessor and
// `[Function: sg_get]` for a static one.
//
// Both come from the fn-name registry, which is where a source
// spelling belongs. The accessor body was kept out of it entirely
// (instance) or entered under its raw symbol (static): the body's
// symbol is `<p>_get` / `<p>_set`, and a property may itself end in
// `_get`, so the key comes from the accessor pair registry rather
// than from the spelling. The row cannot shadow `.name`: the face's
// own name is read first.
//
// A COMPUTED accessor has no source name at all and keeps its miss —
// the same verdict 564-01 gave the computed method face.
const k = "c1";

class C {
  get cg() { return 1 }
  set cg(v: number) {}
  get only_get() { return 2 }
  static get sg() { return 3 }
  static set sg(v: number) {}
  get [k]() { return 4 }
  m() { return 5 }
}

const cd = Object.getOwnPropertyDescriptor(C.prototype, "cg")!;
console.log(JSON.stringify(cd.get!.name), JSON.stringify(cd.set!.name));
console.log(cd.get, cd.set);
console.log(JSON.stringify(cd.get!.length), JSON.stringify(cd.set!.length));

const od = Object.getOwnPropertyDescriptor(C.prototype, "only_get")!;
console.log(JSON.stringify(od.get!.name), od.get, od.set);

const sd = Object.getOwnPropertyDescriptor(C, "sg")!;
console.log(JSON.stringify(sd.get!.name), JSON.stringify(sd.set!.name));
console.log(sd.get, sd.set, JSON.stringify(sd.get!.length));

const kd = Object.getOwnPropertyDescriptor(C.prototype, k)!;
console.log(JSON.stringify(kd.get!.name), kd.get);

// a plain method is untouched, and every face still runs
const c: any = new C();
console.log(JSON.stringify(c.m.name), c.m, c.m());
console.log(c.cg, c.only_get, (C as any).sg, c[k]);
console.log(JSON.stringify(Object.getOwnPropertyNames(C.prototype)));
