// Adapted from test262: throwing a structured value (`throw {message: ...}`)
// + reinterpretation via `catch (e: Err)` annotation drives the reinterpret.
type Err = { code: number, message: string };

function check(): number {
  let code: number = 0;
  let msg: string = "";
  try {
    throw { code: 7, message: "oops" };
  } catch (e: Err) {
    code = e.code;
    msg = e.message;
  }
  if (code !== 7) { throw "#1"; }
  if (msg !== "oops") { throw "#2"; }
  return 0;
}
console.log(check());
