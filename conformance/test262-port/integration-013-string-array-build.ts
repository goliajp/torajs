// Integration: build an array of pre-formatted string parts then join.
// Avoids number→string coercion (tr doesn't auto-coerce; bun does).
function check(): number {
  let parts: string[] = [];
  parts.push("alpha");
  parts.push("beta");
  parts.push("gamma");
  if (parts.length !== 3) { throw "#1"; }
  if (parts.join(",") !== "alpha,beta,gamma") { throw "#2"; }
  if (parts.join("") !== "alphabetagamma") { throw "#3"; }
  if (parts.join(" - ") !== "alpha - beta - gamma") { throw "#4"; }
  return 0;
}
console.log(check());
