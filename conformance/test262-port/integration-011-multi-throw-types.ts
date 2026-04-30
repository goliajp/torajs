// Integration: a fn that throws different value types depending on
// input, with multi-shape catch via `e: T` reinterpretation.
type Err = { code: number, message: string };

function classify(n: number): number {
  if (n < 0) { throw "negative"; }
  if (n === 0) { throw { code: 0, message: "zero" }; }
  if (n > 100) { throw 999; }
  return n * 2;
}

function check(): number {
  if (classify(5) !== 10) { throw "#1"; }

  let s_caught: string = "";
  try { classify(-1); } catch (e: string) { s_caught = e; }
  if (s_caught !== "negative") { throw "#2"; }

  let code: number = -1;
  let msg: string = "";
  try { classify(0); } catch (e: Err) { code = e.code; msg = e.message; }
  if (code !== 0) { throw "#3"; }
  if (msg !== "zero") { throw "#4"; }

  let n_caught: number = 0;
  try { classify(101); } catch (e: number) { n_caught = e; }
  if (n_caught !== 999) { throw "#5"; }

  return 0;
}
console.log(check());
