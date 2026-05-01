// Adapted from test262: switch on string scrutinee — strict-eq dispatch.
function permission(role: string): number {
  switch (role) {
    case "admin": return 3;
    case "editor": return 2;
    case "viewer": return 1;
    default: return 0;
  }
}

function check(): number {
  if (permission("admin") !== 3) { throw "#1"; }
  if (permission("editor") !== 2) { throw "#2"; }
  if (permission("viewer") !== 1) { throw "#3"; }
  if (permission("ghost") !== 0) { throw "#4"; }
  return 0;
}
console.log(check());
