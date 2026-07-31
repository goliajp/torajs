// §22.1.3.23 step 2 — String.prototype.split dispatches a user
// @@split on the separator before any coercion (limit's ToUint32
// included), with «O, limit» passed raw and the separator as this.
const sep: any = {};
sep[Symbol.split] = function (str: any, limit: any) {
  return [str, limit, this === sep];
};
console.log(JSON.stringify("abc".split(sep)));
console.log(JSON.stringify("abc".split(sep, 5)));

// The limit reaches the splitter RAW — no ToUint32 (a valueOf probe
// must not fire).
const probed: string[] = [];
const lim: any = {
  valueOf: function (): number {
    probed.push("valueOf");
    return 2;
  },
};
const sep2: any = {};
sep2[Symbol.split] = function (_s: any, l: any) {
  return l === lim;
};
console.log("abc".split(sep2, lim), probed.length);

// The splitter's return is arbitrary — a non-array passes through.
const sep3: any = {};
sep3[Symbol.split] = function (): number {
  return 42;
};
console.log("abc".split(sep3));

// Probe miss — an object with no @@split falls to ToString(separator).
const plain: any = { toString: () => "b" };
plain["x"] = 1;
console.log(JSON.stringify("abc".split(plain)));

// Present-but-not-callable — GetMethod §7.3.11 step 4 TypeError.
const sep4: any = {};
sep4[Symbol.split] = 7;
try {
  "abc".split(sep4);
  console.log("no-throw");
} catch (e) {
  console.log("not-callable", e instanceof TypeError);
}

// any-receiver lane: the same step-2 dispatch.
const s: any = "xyz";
console.log(JSON.stringify(s.split(sep, 9)));
