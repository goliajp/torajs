// RFC 20260825-injection-reachability 刀 B — every observation door
// in this file is OPEN (try/catch), so the reachability gate keeps
// today's full injection and the caught value is a real instance.
// The skip branch (all doors closed) is exercised by the empty-shape
// programs the deadstrip census measures; THIS fixture pins the
// gate's conservative side.
const n: any = null;
try {
  n.foo();
} catch (e) {
  console.log(e instanceof TypeError);
  console.log(e instanceof Error);
  console.log(typeof (e as Error).message);
}

const xs: any = [1, 2, 3];
try {
  JSON.parse("{bad");
} catch (e) {
  console.log(e instanceof SyntaxError);
}
console.log(xs.length);
