// §21.2.3.{1,2,3} ThisBigIntValue and §20.2.3.5 step 4 — a toString /
// valueOf / toLocaleString minted on BigInt.prototype or
// Function.prototype and borrowed onto a receiver of the wrong brand is a
// TypeError. tr had the rule and applied it to Number and Boolean only,
// so these answered a value where the spec raises.

function probe(label: string, f: () => unknown): void {
  try {
    console.log(label + ": " + String(f()));
  } catch (e) {
    console.log(label + ": " + (e as any).constructor.name);
  }
}

const bip: any = BigInt.prototype;
probe("bigint-toString-on-number", () => bip.toString.call(1));
probe("bigint-toString-on-object", () => bip.toString.call({}));
probe("bigint-valueOf-on-object", () => bip.valueOf.call({}));
probe("bigint-toLocaleString-on-object", () => bip.toLocaleString.call({}));

const fnp: any = Function.prototype;
probe("fn-toString-on-object", () => fnp.toString.call({}));
probe("fn-toString-on-number", () => fnp.toString.call(1));

// Controls — the right brand still answers, on both new families.
probe("bigint-toString-ok", () => bip.toString.call(10n));
probe("bigint-valueOf-ok", () => bip.valueOf.call(10n));
probe("fn-toString-ok", () => fnp.toString.call(function f() {}));

// Controls — the two families that already had the gate are unchanged.
const np: any = Number.prototype;
probe("num-toString-on-object", () => np.toString.call({}));
probe("num-toString-ok", () => np.toString.call(5));
const bp: any = Boolean.prototype;
probe("bool-toString-on-object", () => bp.toString.call({}));
probe("bool-toString-ok", () => bp.toString.call(true));

// Control — toLocaleString joined the mid list, so Number's
// thisNumberValue reading of it (§21.1.3.4) now holds too.
probe("num-toLocaleString-on-object", () => np.toLocaleString.call({}));
probe("num-toLocaleString-ok", () => np.toLocaleString.call(7));

// Control — the ordinary spellings are untouched.
console.log((10n).toString(), (5).toString(), true.toString());
