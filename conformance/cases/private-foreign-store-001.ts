// RFC 20260916-private-element-brand slice 2c — private elements a
// derived class installs on the object its base constructor returned.

function te(f: () => unknown): string {
  try {
    return "value " + String(f());
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other " + String(e);
  }
}

class Base {
  constructor(o: any) {
    return o;
  }
}

class Stamp extends Base {
  #tag = "stamped";
  #count = 0;
  static tag(o: any) { return o.#tag; }
  static bump(o: any) { o.#count = o.#count + 1; return o.#count; }
  static has(o: any) { return #tag in o; }
}

const plain: any = { visible: 1 };
new Stamp(plain);
console.log(Stamp.tag(plain), Stamp.bump(plain), Stamp.bump(plain), Stamp.has(plain));
console.log(JSON.stringify(Object.keys(plain)), Object.getOwnPropertyNames(plain).length);
console.log(te(() => new Stamp(plain)));
console.log(te(() => Stamp.tag({})), Stamp.has({}));

// a method brand is an element too
class Branded extends Base {
  #m() { return 1; }
}
const b: any = {};
new Branded(b);
console.log(te(() => new Branded(b)));

class Getter extends Base {
  get #g() { return 2; }
}
const g: any = [];
new Getter(g);
console.log(te(() => new Getter(g)), Array.isArray(g), g.length);

// on a Proxy: no trap sees the private name
const log: string[] = [];
const target = {};
const handler = {
  get(t: any, k: any) { log.push("get " + String(k)); return t[k]; },
  set(t: any, k: any, v: any) { log.push("set " + String(k)); t[k] = v; return true; },
  defineProperty(t: any, k: any, d: any) { log.push("define " + String(k)); return Reflect.defineProperty(t, k, d); },
};
const p1: any = new Proxy(target, handler);
const p2: any = new Proxy(target, handler);
class Field extends Base {
  #field = 10;
  static gf(o: any) { return o.#field; }
  static sf(o: any) { o.#field = 15; }
}
new Field(p1);
console.log(Field.gf(p1));
Field.sf(p1);
console.log(Field.gf(p1), te(() => Field.gf(p2)), te(() => Field.gf(target)));
console.log(JSON.stringify(log));

// adopting another instance of the same class: it already holds #x
class Twice extends Base {
  #x = 1;
  constructor(o?: any) { super(o); }
  static x(o: any) { return o.#x; }
}
const t1 = new Twice();
console.log(te(() => new Twice(t1)), Twice.x(t1));
