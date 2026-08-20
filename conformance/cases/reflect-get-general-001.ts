// §28.1.6 `Reflect.get(target, key[, receiver])`. tr folded exactly one
// shape — a typed-struct target with a literal string key — and made
// every other shape a compile error, which meant the function did not
// compile on what people reach for it for: `Reflect.get(obj, "a")`
// where `obj` is an `any`. The fold stays because it is better code
// where it applies; the rest takes the general [[Get]] lane.
//
// The receiver is step 3, and it changes exactly one thing: an
// accessor answer runs its getter against it. A data answer is
// whatever the walk found, receiver or no receiver.
function show(label: string, v: any) {
  console.log(label, JSON.stringify(v));
}

// The plain shape that used to be a hard reject.
const plain: any = { a: 1, b: "two" };
show("plain", [Reflect.get(plain, "a"), Reflect.get(plain, "b")]);

// A key that is not a literal.
const k = "a";
show("dynamic-key", [Reflect.get(plain, k), Reflect.get(plain, "a" + "")]);

// An own getter runs against the receiver when one is given.
const own: any = {
  tag: "T",
  get g() {
    return this.tag;
  },
};
const recv: any = { tag: "R" };
show("own-getter", [Reflect.get(own, "g"), Reflect.get(own, "g", recv)]);

// So does an inherited one — the walk finds it on the prototype, the
// receiver is still what it runs against.
const proto: any = {
  get p() {
    return this.tag;
  },
};
const child: any = Object.create(proto);
child.tag = "C";
show("inherited-getter", [Reflect.get(child, "p"), Reflect.get(child, "p", recv)]);

// A data answer ignores the receiver entirely.
show("data-ignores-receiver", [
  Reflect.get(plain, "a"),
  Reflect.get(plain, "a", recv),
]);

// Absent everywhere is undefined, not a throw.
show("absent", [Reflect.get({} as any, "nope"), Reflect.get({} as any, "nope", recv)]);

// Key domain is ToPropertyKey: a symbol stays a symbol, and an object
// key is coerced exactly once.
const sym = Symbol("k");
const symmed: any = { [sym]: 9 };
show("symbol-key", [Reflect.get(symmed, sym)]);

let coercions = 0;
const objKey: any = {
  toString() {
    coercions++;
    return "tag";
  },
};
show("topropertykey", [Reflect.get(recv, objKey), coercions]);

// Array receivers answer their index and length faces.
const arr: any = [10, 20, 30];
show("array", [Reflect.get(arr, "length"), Reflect.get(arr, "2"), Reflect.get(arr, 1 as any)]);

// A class instance keeps the struct lane, fields and accessors alike.
class C {
  x = 5;
  get doubled(): number {
    return this.x * 2;
  }
}
const inst: any = new C();
show("class", [Reflect.get(inst, "x"), Reflect.get(inst, "doubled")]);

// §28.1.6 step 1 — a primitive target is a TypeError, with no ToObject
// wrap to soften it.
show(
  "primitive-target",
  (() => {
    try {
      return Reflect.get(5 as any, "a");
    } catch (e) {
      return (e as Error).constructor.name;
    }
  })()
);

// A getter's own throw propagates unchanged.
show(
  "getter-throws",
  (() => {
    try {
      return Reflect.get(
        {
          get boom(): any {
            throw new RangeError("boom");
          },
        } as any,
        "boom"
      );
    } catch (e) {
      return [(e as Error).constructor.name, (e as Error).message];
    }
  })()
);

// The detached call carries the same shape, and the function's own
// length stays at the spec's 2.
const detached: any = Reflect.get;
show("detached", [detached(own, "g"), detached(own, "g", recv)]);
show("face", [Reflect.get.length, Reflect.get.name]);
