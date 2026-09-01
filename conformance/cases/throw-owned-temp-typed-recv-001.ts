// rotation 550 — a fresh typed receiver (Str / Arr / Map / Set / Date /
// array HOF) held across a throwing argument or callback is released
// on the throw path; a str-method argv temp parked before a later
// throwing argument is released too. Churn probes: 21-387MB → 2MB per
// 600k caught throws. Every shape's normal path still answers.
const s = (n: number): string => "v" + n;
const a = (n: number): number[] => [n, n];
const aa = (n: number): any[] => [n, n];
const m = (n: number): Map<number, number> => new Map([[n, n]]);
const st = (n: number): Set<number> => new Set([n]);
const d = (n: number): Date => new Date(n);
const boom = (): any => {
  throw new Error("x");
};
let caught = 0;
const N = 200;

// 1. Str receiver across a throwing argument; a parked argv temp
//    (`s(i)` as searchValue) across a later throwing argument
for (let i = 0; i < N; i++) {
  try {
    s(i).padStart(boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    s(i).replace(s(i), boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    s(i).concat(boom());
  } catch (e) {
    caught++;
  }
}
console.log(s(1).padStart(4, "-"), s(2).replace(s(2), "w"), s(3).concat("!"));

// 2. Arr receiver across a throwing argument (search / concat lanes)
for (let i = 0; i < N; i++) {
  try {
    a(i).indexOf(boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    a(i).concat(boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    aa(i).includes(boom());
  } catch (e) {
    caught++;
  }
}
console.log(a(4).indexOf(4), a(5).concat([6]).length, aa(7).includes(7));

// 3. Arr receiver into the HOF / predicate / flatMap lanes — normal
//    path only here: a callback that throws mid-loop is 550-01
//    (the may-throw gate misses a let-bound arrow callee and flatMap
//    has no ReturnIfAbrupt at all), fixture to follow with that fix.
console.log(
  a(8).map((x: number): number => x * 2),
  a(9).find((x: number): boolean => x > 0),
  a(10).flatMap((x: number): number[] => [x, x]).length,
);

// 4. Map / Set / Date receivers across a throwing argument
for (let i = 0; i < N; i++) {
  try {
    m(i).set(1, boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    m(i).get(boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    st(i).add(boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    d(i).setTime(boom());
  } catch (e) {
    caught++;
  }
}
console.log(m(11).set(1, 2).size, m(12).get(12), st(13).add(14).size, d(15).setTime(16));

console.log(caught);
