// Function declarations nested in a generator or async body capture the
// state machine's frame, and §10.2.11 binds them before the body runs:
// peers see each other and a call above the declaration finds it live.

function* gen(n: number) {
  let t = 0;
  function e(m: number): number { t++; return m === 0 ? t : o(m - 1); }
  function o(m: number): number { t++; return m === 0 ? -t : e(m - 1); }
  yield e(n);
  yield o(n);
}
for (const v of gen(3)) console.log(v);

function* single(n: number) {
  let t = 0;
  function e(m: number): number { t++; return m + t; }
  yield e(n);
  yield e(n);
}
for (const v of single(3)) console.log(v);

const ge = function* (n: number) {
  let t = 0;
  yield e(n);
  function e(m: number): number { t++; return m === 0 ? t : o(m - 1); }
  function o(m: number): number { t++; return m === 0 ? -t : e(m - 1); }
};
for (const v of ge(4)) console.log(v);

class W {
  *walk(n: number) {
    let s = 0;
    yield step(n);
    yield step(n + 1);
    function step(m: number): number { s += m; return m === 0 ? s : back(m - 1); }
    function back(m: number): number { s -= 1; return m === 0 ? -s : step(m - 1); }
  }
}
for (const v of new W().walk(2)) console.log(v);

async function run(n: number): Promise<number> {
  let t = 0;
  const first = e(n);
  await null;
  return first + o(n);
  function e(m: number): number { t++; return m === 0 ? t : o(m - 1); }
  function o(m: number): number { t++; return m === 0 ? -t : e(m - 1); }
}

const arun = async (n: number): Promise<number> => {
  let t = 1;
  await null;
  return e(n);
  function e(m: number): number { t *= 2; return m === 0 ? t : o(m - 1); }
  function o(m: number): number { t += 1; return m === 0 ? -t : e(m - 1); }
};

async function* ag(n: number) {
  let t = 0;
  yield e(n);
  await null;
  yield o(n);
  function e(m: number): number { t++; return m === 0 ? t : o(m - 1); }
  function o(m: number): number { t++; return m === 0 ? -t : e(m - 1); }
}

(async () => {
  console.log(await run(3));
  console.log(await arun(4));
  for await (const v of ag(3)) console.log(v);
})();
