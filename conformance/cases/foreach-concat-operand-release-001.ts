// rotation 552 — Map / Set forEach pass the receiver to the callback by
// borrow (the per-iteration transfer inc leaked the whole receiver on
// every walk, 387MB / 600k) and park the callback env for the callback's
// throw path; Str.concat releases its owned-temp arguments and parks a
// fresh running product across a later argument's lower (38-40MB /
// 600k). Churn probes on mini: all shapes 1.8-2.2MB after. Every shape
// answers what bun answers.
const s = (n: number): string => "v" + n;
const boom = (): any => {
  throw new Error("x");
};
const N = 200;
let caught = 0;

// 1. Map.forEach — callback arity 1 / 2 / 3; the receiver survives
//    through the third parameter
const m = (n: number): Map<number, string> => new Map([[n, s(n)], [n + 1, s(n + 1)]]);
let acc = 0;
for (let i = 0; i < N; i++) {
  m(i).forEach((v: string): void => {
    acc += v.length;
  });
}
console.log("map1", acc);
acc = 0;
for (let i = 0; i < N; i++) {
  m(i).forEach((v: string, k: number): void => {
    acc += k;
  });
}
console.log("map2", acc);
let keep: Map<number, string> | undefined;
for (let i = 0; i < N; i++) {
  m(i).forEach((v: string, k: number, mm: Map<number, string>): void => {
    keep = mm;
  });
}
if (keep) console.log("map3", keep.size, [...keep.keys()].join(","), keep.get(N));

// 2. Map.forEach — the callback throws on the first entry
for (let i = 0; i < N; i++) {
  try {
    m(i).forEach((v: string, k: number): void => {
      boom();
    });
  } catch (e) {
    caught++;
  }
}
console.log("map-throw", caught);

// 3. Set.forEach — normal walk and a throwing callback
const st = (n: number): Set<string> => new Set([s(n), s(n + 1)]);
acc = 0;
for (let i = 0; i < N; i++) {
  st(i).forEach((v: string): void => {
    acc += v.length;
  });
}
console.log("set1", acc);
for (let i = 0; i < N; i++) {
  try {
    st(i).forEach((v: string): void => {
      boom();
    });
  } catch (e) {
    caught++;
  }
}
console.log("set-throw", caught);

// 4. Str.concat — owned-temp arguments, literals, a view, an Any, an
//    explicit undefined, and a throw in the second / third argument
let out = "";
for (let i = 0; i < N; i++) {
  out = s(i).concat(s(i), s(i));
}
console.log("cat1", out);
console.log("cat2", "q".concat(s(1), "-", s(2)[0], String(3)));
console.log("cat3", "c".concat(Object(5)));
console.log("cat4", "a".concat(undefined));
for (let i = 0; i < N; i++) {
  try {
    s(i).concat(s(i), String(boom()));
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    s(i).concat("a", String(boom()));
  } catch (e) {
    caught++;
  }
}
console.log("cat-throw", caught);
