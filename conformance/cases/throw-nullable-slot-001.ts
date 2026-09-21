// A pointer-shaped slot holding the generic immortal cell is
// `undefined`, and the throw lane tagged every refcounted pointer
// ANY_HEAP regardless — so the catch binding got a heap value whose
// `typeof` was "object".

const os: { a: number }[] = [];
const miss = os.find((x) => true);
try {
  throw miss;
} catch (e) {
  console.log(typeof e, e);
}

const q: { a: number } | undefined = undefined;
try {
  throw q;
} catch (e) {
  console.log(typeof e, e);
}

const xs: number[] | undefined = undefined;
try {
  throw xs;
} catch (e) {
  console.log(typeof e, e);
}

// --- the live side of the same slot is thrown as itself
const q2: { a: number } | undefined = { a: 1 };
try {
  throw q2;
} catch (e) {
  console.log(typeof e, JSON.stringify(e));
}
const hit = [[1]].find((x) => true);
try {
  throw hit;
} catch (e) {
  console.log(typeof e, JSON.stringify(e));
}

// --- the spellings that were already right
try {
  throw undefined;
} catch (e) {
  console.log(typeof e, e);
}
try {
  throw null;
} catch (e) {
  console.log(typeof e, e);
}
try {
  throw { a: 1 };
} catch (e) {
  console.log(typeof e, JSON.stringify(e));
}
try {
  throw "s";
} catch (e) {
  console.log(typeof e, e);
}
try {
  throw 7;
} catch (e) {
  console.log(typeof e, e);
}
