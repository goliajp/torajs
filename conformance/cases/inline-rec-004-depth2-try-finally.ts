// Self-recursion through try/finally: every level's finally runs once,
// innermost first, whether the bottom returns or throws.
const log: string[] = [];

function down(n: number, fail: boolean): number {
  try {
    if (n === 0) {
      if (fail) throw new Error("bottom");
      return 0;
    }
    return 1 + down(n - 1, fail);
  } finally {
    log.push("f" + n);
  }
}

console.log(down(4, false));
console.log(log.join(" "));
log.length = 0;
try {
  down(5, true);
} catch (e) {
  console.log("caught", (e as Error).message);
}
console.log(log.join(" "));

function guarded(n: number): number {
  if (n === 0) throw new Error("zero");
  try {
    return guarded(n - 1);
  } catch (e) {
    return n * 10;
  } finally {
    log.push("g" + n);
  }
}

log.length = 0;
console.log(guarded(3));
console.log(log.join(" "));
