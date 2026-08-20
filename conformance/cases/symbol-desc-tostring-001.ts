// §20.4.1.1 step 3 — the Symbol description is `? ToString(description)`,
// not a string-shaped operand. `Symbol(1)` describes itself "1"; a Symbol
// description throws TypeError (§7.1.17 has no Symbol arm) and the throw
// is catchable, so the program still has to compile.
const n = Symbol(1);
console.log(String(n), n.description);

const nul = Symbol(null);
console.log(String(nul), nul.description);

const b = Symbol(true);
console.log(String(b), b.description);

// `undefined` is step 2 — no description at all, not the string
// "undefined". The operand still evaluates for its side effect.
let ticks = 0;
function tick(): undefined {
  ticks++;
  return undefined;
}
const u = Symbol(tick());
console.log(String(u), u.description, ticks);

// OrdinaryToPrimitive with hint string: toString first, valueOf as the
// fallback when toString answers a non-primitive.
let calls = "";
const both = {
  toString() {
    calls += "toString";
    return {};
  },
  valueOf() {
    calls += "valueOf";
    return "V";
  },
};
console.log(String(Symbol(both)), calls);

calls = "";
const noToString = {
  toString: null,
  valueOf() {
    calls += "valueOf";
    return "W";
  },
};
console.log(String(Symbol(noToString)), calls);

// Both hooks answering objects is the §7.1.1 TypeError, after both ran.
calls = "";
const neither = {
  toString() {
    calls += "toString";
    return {};
  },
  valueOf() {
    calls += "valueOf";
    return {};
  },
};
try {
  Symbol(neither);
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name, calls);
}

// A hook's own throw propagates unchanged — it is not remapped.
try {
  Symbol({
    toString() {
      throw new RangeError("boom");
    },
  });
} catch (e) {
  console.log((e as Error).constructor.name, (e as Error).message);
}

// ToString(Symbol) throws TypeError (built-ins/Symbol/desc-to-string-symbol).
try {
  Symbol(n);
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}

// The Any lane takes the same route.
const anyNum: any = 7;
console.log(String(Symbol(anyNum)));
const anySym: any = Symbol("q");
try {
  Symbol(anySym);
  console.log("no throw");
} catch (e) {
  console.log((e as Error).constructor.name);
}

// Aggregate shapes stringify as they do everywhere else.
console.log(String(Symbol([1, 2])), String(Symbol({})));

// Trailing args past the description still evaluate and are dropped.
let trailing = 0;
const t = Symbol("keep", ((trailing = 1), 2) as any);
console.log(String(t), trailing);
