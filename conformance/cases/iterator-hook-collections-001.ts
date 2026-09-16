// §24.1.1.1 / §24.2.1.1 walk the Map / Set constructor's argument with
// GetIterator. The array-shaped fast paths read its slots instead, so
// an installed hook was invisible to `new Set(arr)` while `[...arr]`
// — same array, same hook — honoured it.

const a = [1, 2, 3];
(a as any)[Symbol.iterator] = function () {
  let i = 0;
  return {
    next: function () {
      return i < 2 ? { value: 60 + i++, done: false } : { value: undefined, done: true };
    },
  };
};

const s = new Set(a as any);
console.log(s.size, JSON.stringify(Array.from(s as any)));

const pairs: any = [[1, "a"], [2, "b"], [3, "c"]];
pairs[Symbol.iterator] = function () {
  let i = 0;
  const rows = [[9, "z"]];
  return {
    next: function () {
      return i < rows.length ? { value: rows[i++], done: false } : { value: undefined, done: true };
    },
  };
};
const m = new Map(pairs as any);
console.log(m.size, JSON.stringify(Array.from((m as any).keys())));

// the literal-initializer lanes stand down with them — an array
// literal is iterated the same way
const s2 = new Set([1, 2, 3]);
console.log(s2.size, JSON.stringify(Array.from(s2 as any)));
const m2 = new Map([[1, "x"], [2, "y"]]);
console.log(m2.size, JSON.stringify(Array.from((m2 as any).values())));
