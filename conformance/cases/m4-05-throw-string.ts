function check(b: boolean): string {
  if (b) { throw "from inner"; }
  return "ok";
}
function safe(b: boolean): string {
  try { return check(b); } catch (e: string) { return e + " caught"; }
}
console.log(safe(false));
console.log(safe(true));
