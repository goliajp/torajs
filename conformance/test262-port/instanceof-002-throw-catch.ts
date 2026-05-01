// Adapted from test262 try-catch with class throw values + instanceof
// narrowing. throws struct-typed values, catch binds them by class
// name, and the body uses `instanceof` to dispatch handlers. tr's
// catch already supports typed `(e: ClassName)` binding; instanceof
// inside the body lets a single catch handle multiple class types
// when paired with `(e: any)` or a top-level union — modeled here
// with two separate try blocks because tr's catch type is currently
// monomorphic.
class NotFoundErr {
  path: string;
  constructor(p: string) { this.path = p; }
}

class PermissionErr {
  user: string;
  constructor(u: string) { this.user = u; }
}

function readFile(path: string, allowed: boolean): string {
  if (!allowed) { throw new PermissionErr("alice"); }
  if (path === "missing.txt") { throw new NotFoundErr(path); }
  return "ok";
}

function check(): number {
  // #1 — NotFound path: catch binds typed; instanceof confirms.
  let nf_handled = false;
  try {
    readFile("missing.txt", true);
  } catch (e: NotFoundErr) {
    if (!(e instanceof NotFoundErr)) { throw "#1a: instanceof in catch"; }
    if (e instanceof PermissionErr) { throw "#1b: cross-class false positive"; }
    if (e.path !== "missing.txt") { throw "#1c: field accessible"; }
    nf_handled = true;
  }
  if (!nf_handled) { throw "#1d: catch did not run"; }

  // #2 — Permission path: same shape, different class.
  let pe_handled = false;
  try {
    readFile("ok.txt", false);
  } catch (e: PermissionErr) {
    if (!(e instanceof PermissionErr)) { throw "#2a"; }
    if (e instanceof NotFoundErr) { throw "#2b"; }
    if (e.user !== "alice") { throw "#2c"; }
    pe_handled = true;
  }
  if (!pe_handled) { throw "#2d"; }

  // #3 — Happy path: no throw.
  let r = readFile("ok.txt", true);
  if (r !== "ok") { throw "#3"; }
  return 0;
}
console.log(check());
