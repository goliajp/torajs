// §7.4.4 / §7.4.5 — the IteratorResult reads are a real [[Get]]:
// struct-probe fast lane, accessor-aware dispatcher lane (shared
// iter_result_get, both step tiers). test262's iter-val-err family
// poisons the step's `value` with a throwing getter and never says
// done — the ONLY loop exit is the abrupt completion, so a raw
// field probe (no getter fire, no throw) spun forever.

// 1) poisoned value getter — the throw is the exit
const poisoned = Object.defineProperty({}, 'value', {
  get: function () {
    throw new Error('boom');
  },
});
const iter: any = {};
iter[Symbol.iterator] = function () {
  return {
    next: function () {
      return poisoned;
    },
  };
};
try {
  for (const [...x] of [iter]) {
    break;
  }
} catch (e) {
  console.log('caught', (e as any).message);
}

// 2) truthy non-boolean done terminates (§7.4.4 ToBoolean)
const one: any = {};
one[Symbol.iterator] = function () {
  let i = 0;
  return {
    next: function () {
      i += 1;
      return { value: 10 * i, done: i > 2 ? 1 : 0 };
    },
  };
};
const [a, b, ...rest] = one;
console.log(a, b, rest.length);

// 3) plain object-literal steps keep working (regression face).
// (The exhausted step answers `value: 0`, not `undefined` — a
// ternary/return unify of Struct([value: Number]) with
// Struct([value: Undefined]) is a recorded pre-existing checker
// wall, S2.27 at struct-field depth; not this fixture's face.)
const plain: any = {};
plain[Symbol.iterator] = function () {
  let i = 0;
  return {
    next: function () {
      i += 1;
      if (i <= 3) {
        return { value: i, done: false };
      }
      return { value: 0, done: true };
    },
  };
};
const [...all] = plain;
console.log(all.length, all[0], all[2]);
