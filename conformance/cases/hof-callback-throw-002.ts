// 550-01 — a callback that throws mid-loop ends the array HOF and
// the throw reaches the enclosing catch EVERY time. Two pre-existing
// misses: the may-throw gate resolved a let-bound arrow callee
// (`const boom = () => { throw … }`) by its binding name and never saw
// the lifted decl, so a devirt'd callback whose only throw source was
// `boom()` was judged never-throwing and the loop's check was pruned
// (the pending throw then strayed into the NEXT checked call — one
// catch in three); and flatMap had no ReturnIfAbrupt at all (walked
// the ret sentinel as an array — exit 139).
const a = (n: number): number[] => [n, n + 1, n + 2];
const s = (n: number): string => "v" + n;
const boom = (): any => {
  throw new Error("x");
};
let caught = 0;
const N = 3;

// 1. devirt'd inline callbacks whose only throw source is `boom()`
for (let i = 0; i < N; i++) {
  try {
    a(i).map((x: number): number => x + boom());
  } catch (e) {
    caught++;
    console.log("map", i, (e as Error).message);
  }
}
for (let i = 0; i < N; i++) {
  try {
    a(i).filter((x: number): boolean => x > boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    a(i).forEach((x: number): void => {
      boom();
    });
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    a(i).reduce((acc: number, x: number): number => acc + boom(), 0);
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    a(i).find((x: number): boolean => x > boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    a(i).some((x: number): boolean => x > boom());
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    a(i).flatMap((x: number): number[] => [x, boom()]);
  } catch (e) {
    caught++;
  }
}

// 2. the throw lands on the second element — the partially built
//    product (refcounted elements / string accumulator) is released
const xs = [1, 2, 3];
for (let i = 0; i < N; i++) {
  try {
    xs.map((x: number): string => (x === 2 ? boom() : s(x)));
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    xs.reduce((acc: string, x: number): string => (x === 2 ? boom() : acc + s(x)), "");
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    xs.flatMap((x: number): string[] => (x === 2 ? boom() : [s(x), s(x)]));
  } catch (e) {
    caught++;
  }
}

// 3. a bound (non-devirt) callback and a callback calling a named fn
const cb = (x: number): number => x + boom();
for (let i = 0; i < N; i++) {
  try {
    a(i).map(cb);
  } catch (e) {
    caught++;
  }
}
function boomNamed(): number {
  throw new Error("y");
}
for (let i = 0; i < N; i++) {
  try {
    a(i).map((x: number): number => x + boomNamed());
  } catch (e) {
    caught++;
    console.log("named", i, (e as Error).message);
  }
}

// normal paths
console.log(
  a(1).map((x: number): number => x * 2),
  a(1).filter((x: number): boolean => x > 1),
  a(1).reduce((acc: number, x: number): number => acc + x, 0),
  a(1).find((x: number): boolean => x > 1),
  a(1).some((x: number): boolean => x > 2),
  a(1).flatMap((x: number): number[] => [x, x]).length,
  xs.map((x: number): string => s(x)),
  xs.reduce((acc: string, x: number): string => acc + s(x), ""),
);
console.log(caught);
