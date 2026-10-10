// An `any` value that is an object but no class instance (a JSON.parse
// result, an object literal held as `any`), written into a slot the
// program annotated with a class or an object type, is checked against
// the type's fields and copied into a fresh cell of the slot's layout:
// a class slot then holds an instance of the class, whose methods run
// on it. Other properties, their order and whether something else holds
// the object do not matter; a missing field or a field of another type
// throws a TypeError. bun keeps the object as it is (no methods, no
// instance, one object behind both names), so the lines that differ
// are checked against the .expected.

class Cfg {
  name: string;
  port: number;
  debug: boolean;
  constructor(name: string, port: number, debug: boolean) {
    this.name = name;
    this.port = port;
    this.debug = debug;
  }
  url(): string {
    return this.name + ":" + this.port;
  }
}
class Inner {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  twice(): number {
    return this.v * 2;
  }
}
class Outer {
  label: string;
  inner: Inner;
  constructor(label: string, inner: Inner) {
    this.label = label;
    this.inner = inner;
  }
}
class Opt {
  a: number;
  b?: string;
  c: Inner | null;
  constructor() {
    this.a = 0;
    this.c = null;
  }
}
class Bag {
  tags: string[];
  items: Inner[];
  constructor() {
    this.tags = [];
    this.items = [];
  }
}
type Pt = { x: number; y: number };

function attempt(tag: string, f: () => void): void {
  try {
    f();
    console.log(tag, "ok");
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message);
  }
}
function show(c: Cfg): string {
  return c.name + ":" + c.port + " " + c.debug;
}
// what only an instance answers (bun: not one)
function inst(c: Cfg): string {
  return (c instanceof Cfg) + " " + c.url();
}
const text = '{"name":"db","port":5432,"debug":true}';

// a let, with the fields in another order and other properties beside
attempt("let", () => {
  const c1: Cfg = JSON.parse(text);
  console.log(show(c1));
  const c2: Cfg = JSON.parse('{"debug":false,"extra":1,"port":80,"name":"web"}');
  console.log(show(c2));
  console.log(inst(c1), inst(c2));
});

// an object literal through `any`, still held by its own name: the slot
// takes a copy
const lit: any = { port: 1, more: [1], name: "lit", debug: false };
attempt("literal", () => {
  const c3: Cfg = lit;
  console.log(show(c3));
  lit.name = "changed";
  console.log(c3.name, lit.name, c3 === lit);
});

// a field, a return, an append to an annotated array
class Box {
  cfg: Cfg;
  constructor() {
    this.cfg = new Cfg("x", 0, false);
  }
}
attempt("field", () => {
  const b = new Box();
  b.cfg = JSON.parse(text);
  console.log(show(b.cfg));
  console.log(inst(b.cfg));
});
function load(t: string): Cfg {
  return JSON.parse(t);
}
attempt("return", () => console.log(show(load(text)), inst(load(text))));
attempt("push", () => {
  const list: Cfg[] = [];
  list.push(JSON.parse(text));
  list.push({ name: "p", port: 2, debug: true } as any);
  console.log(list.map(show).join(" | "));
});

// a parameter reached through `any`
attempt("param", () => {
  const f: any = show;
  console.log(f(JSON.parse(text)));
});

// an accessor answers through [[Get]]
attempt("getter", () => {
  const g: any = {
    get name() {
      return "got";
    },
    port: 3,
    debug: true,
  };
  const c: Cfg = g;
  console.log(show(c));
});

// nested object, optional and nullable fields, arrays
attempt("nested", () => {
  const o: Outer = JSON.parse('{"inner":{"v":21},"label":"o"}');
  console.log(o.label, o.inner.twice(), o.inner instanceof Inner, o instanceof Outer);
});
attempt("optional", () => {
  const p1: Opt = JSON.parse('{"a":1,"c":null}');
  console.log(p1.a, p1.b, p1.c);
  const p2: Opt = JSON.parse('{"a":2,"b":"x","c":{"v":3}}');
  console.log(p2.a, p2.b, p2.c === null ? "null" : p2.c.twice());
});
attempt("arrays", () => {
  const bag: Bag = JSON.parse('{"tags":["a","b"],"items":[{"v":1},{"v":2}]}');
  console.log(bag.tags.join(","), bag.items.map((i) => i.twice()).join(","));
  console.log(bag.items[0] instanceof Inner);
});
attempt("class array", () => {
  const cs: Cfg[] = JSON.parse("[" + text + "," + text + "]");
  console.log(cs.length, show(cs[1]));
  console.log(inst(cs[0]));
});
attempt("object type", () => {
  const p: Pt = JSON.parse('{"y":2,"x":1,"z":3}');
  console.log(p.x, p.y);
});

// a class constructed through a value casts its class-typed parameter
class Holder {
  c: Cfg;
  constructor(c: Cfg) {
    this.c = c;
  }
}
const HH: any = Holder;
attempt("ctor param", () => {
  const h = new HH(JSON.parse(text));
  console.log(show(h.c));
  console.log(inst(h.c));
});

// a missing field or a field of another type throws
attempt("missing", () => {
  const c: Cfg = JSON.parse('{"name":"a","port":1}');
});
attempt("wrong type", () => {
  const c: Cfg = JSON.parse('{"name":"a","port":"1","debug":true}');
});
attempt("nested wrong type", () => {
  const o: Outer = JSON.parse('{"label":"o","inner":{"v":"x"}}');
});
attempt("element wrong type", () => {
  const bag: Bag = JSON.parse('{"tags":["a"],"items":[{"v":1},{"w":2}]}');
});
attempt("literal missing", () => {
  const c: Cfg = { name: "a" } as any;
});
attempt("return missing", () => load('{"port":1}'));
attempt("ctor param missing", () => new HH({ port: 1 }));
attempt("not an object", () => {
  const c: Cfg = 3 as any;
});
