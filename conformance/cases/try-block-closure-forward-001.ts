// each block of a try statement creates its lexical bindings on entry,
// like any block: a closure written ahead of a declaration in the same
// block reads the binding the declaration makes
function inTry(): void {
  try {
    const g = () => y;
    const y = 5;
    console.log("t", g());
  } catch (e) {
    console.log("unexpected");
  }
}
function inCatch(): void {
  try {
    throw new Error("x");
  } catch (e) {
    const g = () => m + ":" + (e as Error).message;
    const m = "caught";
    console.log(g());
  }
}
function inFinally(): void {
  try {
    console.log("body");
  } finally {
    const f = () => n * 2;
    const n = 21;
    console.log("f", f());
  }
}
function deadZone(): void {
  try {
    const g = () => z;
    try { g(); } catch (e: any) { console.log("dz", e.constructor.name); }
    const z = 1;
    console.log("z", g());
  } catch (e) {
    console.log("unexpected");
  }
}
inTry();
inCatch();
inFinally();
deadZone();
