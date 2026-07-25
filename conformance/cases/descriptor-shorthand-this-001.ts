// An accessor written as a METHOD SHORTHAND inside a property
// descriptor:
//
//   Object.defineProperty(o, "y", { get() { return this._y + 1 } })
//
// §10.1.7.3 binds an accessor's `this` to the property's receiver, not
// to the descriptor object it was written in. There is a pass that
// says exactly that — it re-anns such a face's receiver to `any` and
// marks it receiver-first — and it already handled this shape. It just
// never ran: its early-out asked whether the program contained any
// function EXPRESSION, and a shorthand is not one. A file whose only
// face is a shorthand skipped the pass entirely and the getter body
// typed `this` as the descriptor:
//
//   no member `._y` on type Struct([("get", Function([], Number))])
//
// Spelling the same getter `get: function () {...}` worked, which is
// what made it look like a shorthand problem rather than a gating one.

function definePropertyGetter(): void {
  const o = { _y: 1 };
  Object.defineProperty(o, "y", {
    get(): number {
      return this._y + 1;
    },
  });
  console.log((o as any).y);
}

function definePropertyAccessorPair(): void {
  const o = { _y: 1 };
  Object.defineProperty(o, "y", {
    get(): number {
      return this._y + 1;
    },
    set(v: number): void {
      this._y = v;
    },
  });
  const a = o as any;
  console.log(a.y);
  a.y = 10;
  console.log(a.y, o._y);
}

function definePropertiesNested(): void {
  const o = { _a: 1, _b: 2 };
  Object.defineProperties(o, {
    a: {
      get(): number {
        return this._a * 10;
      },
    },
    b: {
      get(): number {
        return this._b * 100;
      },
    },
  });
  const x = o as any;
  console.log(x.a, x.b);
}

function objectCreateNested(): void {
  const proto = { _v: 5 };
  const o = Object.create(proto, {
    v: {
      get(): number {
        return this._v + 1;
      },
    },
  });
  console.log((o as any).v);
}

// unchanged ground: the fn-expr spelling of the same face, which is
// what the pass was written for
function fnExprFace(): void {
  const o = { _y: 1 };
  Object.defineProperty(o, "y", {
    get: function (): number {
      return this._y + 1;
    },
  });
  console.log((o as any).y);
}

// a descriptor whose getter never says `this` keeps the plain closure
// ABI — no receiver is passed to it
function thisFreeFace(): void {
  const o = { _y: 1 };
  Object.defineProperty(o, "y", {
    get(): number {
      return 42;
    },
  });
  console.log((o as any).y);
}

// and an ordinary object-literal method in the same program still
// binds `this` to ITS literal, which is the receiver there
function ordinaryMethodUnaffected(): void {
  const o = {
    v: 3,
    m(): number {
      return this.v * 2;
    },
  };
  const p = { _y: 1 };
  Object.defineProperty(p, "y", {
    get(): number {
      return this._y + 1;
    },
  });
  console.log(o.m(), (p as any).y);
}

function main(): void {
  definePropertyGetter();
  definePropertyAccessorPair();
  definePropertiesNested();
  objectCreateNested();
  fnExprFace();
  thisFreeFace();
  ordinaryMethodUnaffected();
}

main();
