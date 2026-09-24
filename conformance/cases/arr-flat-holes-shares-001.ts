// §23.1.3.13.1 FlattenIntoArray: an index HasProperty answers false for
// is skipped, whichever representation the hole has (a `new Array(n)`
// slot, a literal elision, a delete). And the typed flat product takes
// its own share of every element: dropping it used to release the
// source's strings and objects (the source then read freed memory).
function main() {
  const inner1: any[] = [{ a: 1 }, "s" + String(1 + 1)];
  const inner2: any[] = [[1, 2]];
  const ou = [inner1, inner2];
  for (let i = 0; i < 5; i++) { const r = ou.flat(); r.length; }
  console.log(JSON.stringify(inner1[0]), inner1[1], JSON.stringify(inner2), JSON.stringify(ou.flat()));
  const so = [["ab" + "c", "de" + "f"], ["gh" + "i"]];
  for (let i = 0; i < 5; i++) { const r = so.flat(); r.length; }
  console.log(JSON.stringify(so));
  const h = [new Array(2), [1, , 3]];
  const hf = h.flat();
  console.log(hf.length, JSON.stringify(hf));
  const d: any[] = [4, 5, 6];
  delete d[1];
  console.log(JSON.stringify([d, [7]].flat()));
  const x: any = new Array(3);
  x[1] = "mid";
  console.log(JSON.stringify([x, [8, , 9]].flat()), [[, ,], [, ,]].flat().length);
  console.log(JSON.stringify([[1, [2, , 3]], , [4]].flat(2)));
  const o: any[] = [[1], [2], [3]];
  delete o[1];
  console.log(JSON.stringify(o.flat()), JSON.stringify([[1, o]].flat(2)));
}
main();
