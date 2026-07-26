// RFC 20260727 blade 3 — the `await` half of sibling 001.
//
// `await` does not go through the .then kernels; it extracts the value
// directly and casts it by the STATIC inner type. So it had the same
// defect from the same cause: a cell settled from an `any` holds a
// NaN-box pointer, and the cast reinterpreted it — NaN for the number
// lane, a wild deref for the string one.
//
// Kept apart from 001 rather than appended to it: a sync-registered
// `.then` and an async function interleave their microtasks
// differently from bun, so mixing the two in one file compares
// ordering rather than the read-back this fixture is about.

async function awaitLanes() {
  const n: any = 42;
  const pn: Promise<number> = Promise.resolve(n);
  console.log("await-num", await pn);

  const s: any = "s";
  const ps: Promise<string> = Promise.resolve(s);
  console.log("await-str", await ps);

  const b: any = true;
  const pb: Promise<boolean> = Promise.resolve(b);
  console.log("await-bool", await pb);

  // the lane IS `any` — the box is what the site wants, untouched
  const pa: Promise<any> = Promise.resolve(n);
  console.log("await-any", await pa);

  // and a cell whose form already agrees with its lane must not move
  const plain: Promise<number> = Promise.resolve(9);
  console.log("await-plain", await plain);

  // no annotation on the binding — the handler/lane is still number
  const inferred = Promise.resolve(n);
  console.log("await-inferred", await inferred);
}

awaitLanes();
