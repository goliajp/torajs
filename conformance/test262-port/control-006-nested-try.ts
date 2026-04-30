// Adapted from test262: language/statements/try/* — inner try
// throws past its outer try's catch.
function check(): number {
  let log: number = 0;
  try {
    try {
      throw 1;
    } catch (e: number) {
      log = log + e;       // log = 1
      throw e + 10;        // rethrow as 11
    }
  } catch (e: number) {
    log = log + e;          // log = 1 + 11 = 12
  }
  if (log !== 12) { throw "#1"; }
  return 0;
}
console.log(check());
