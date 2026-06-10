// same-scope `let t = s` shares ownership (retain at the binding
// site): both bindings stay fully usable and either may be returned.
function pickSource(): string {
  const s: string = "share-source";
  const t: string = s;
  console.log(t);
  return s;
}

function pickCopy(): string {
  const s: string = "share-copy";
  const t: string = s;
  return t;
}

console.log(pickSource());
console.log(pickCopy());
