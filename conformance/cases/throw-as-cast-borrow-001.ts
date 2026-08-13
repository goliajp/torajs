// Twin of return-as-cast-borrow-001 at the throw site: `throw x as any`
// must judge ownership by the inner read. Judging the `As` node left an
// owned local unmarked for the fn-exit drop walk while the throw slot
// still held it, so the catch read back whatever the churn loop below
// had since allocated onto that page.
function boom(): void {
  const s = "ab" + "cd";
  throw s as any;
}

try {
  boom();
} catch (e) {
  for (let i = 0; i < 200; i++) {
    const junk = "y" + i;
    if (junk === "never") console.log(junk);
  }
  console.log(e);
}
