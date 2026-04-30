// Adapted from test262: language/statements/const/* — const declaration
// rejects re-assignment at typecheck (verified via the negative-test
// surface: `bun` errors and tr also errors). Here we exercise the
// happy path: const reads + initialization.
function check(): number {
  const PI: number = 314;
  const NAME: string = "torajs";
  if (PI !== 314) { throw "#1"; }
  if (NAME !== "torajs") { throw "#2"; }
  if (NAME.length !== 6) { throw "#3"; }
  return 0;
}
console.log(check());
