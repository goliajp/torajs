// Rotation 207 — ES §22.1.3.20 step 2.b (replaceAll) / §22.1.3.14
// step 2.b (matchAll): a RegExp searchValue without the `g` flag is
// a TypeError, and that gate fires BEFORE step 3's ToString(this)
// and step 6.a's ToString(replaceValue). A user `toString` on the
// receiver or on the replacement must therefore never run when the
// search argument already disqualifies the call.

let calls = 0;
let poison = {
  toString() {
    calls += 1;
    throw "poison toString must not run";
  },
};

// replaceAll — poisoned receiver AND poisoned replaceValue; the
// flags gate must fire first, so neither hook runs.
calls = 0;
try {
  "".replaceAll.call(poison, /./, poison);
  console.log("A no-throw");
} catch (e) {
  console.log("A", e instanceof TypeError);
}
console.log("A calls", calls);

// replaceAll — string receiver, poisoned replaceValue only.
calls = 0;
try {
  "".replaceAll.call("aXa", /x/, poison);
  console.log("B no-throw");
} catch (e) {
  console.log("B", e instanceof TypeError);
}
console.log("B calls", calls);

// matchAll — same spec shape, poisoned receiver.
calls = 0;
try {
  "".matchAll.call(poison, /x/);
  console.log("C no-throw");
} catch (e) {
  console.log("C", e instanceof TypeError);
}
console.log("C calls", calls);

// A global RegExp is not gated — the receiver's toString still runs
// exactly once and the replacement happens.
let box = {
  toString() {
    calls += 1;
    return "axbxc";
  },
};
calls = 0;
console.log("D", "".replaceAll.call(box, /x/g, "-"));
console.log("D calls", calls);

// Static lane keeps its own non-global rejection.
try {
  console.log("E no-throw", "axbxc".replaceAll(/x/, "-"));
} catch (e) {
  console.log("E", e instanceof TypeError);
}
console.log("F", "axbxc".replaceAll(/x/g, "-"));
console.log("G", "axbxc".replaceAll("x", "-"));

// matchAll on a string receiver, non-global, still throws.
try {
  "aXa".matchAll(/x/);
  console.log("H no-throw");
} catch (e) {
  console.log("H", e instanceof TypeError);
}

// replace (no `g` requirement) is untouched by the gate.
console.log("I", "axbxc".replace(/x/, "-"));
