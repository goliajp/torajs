// The third boundary of the same Any-to-typed table as
// anylane-number-annotation-001 / anylane-string-annotation-001:
// storing an `any` into a typed array's element slot. Without the
// decode the NaN-box bits ARE the element.

function main() {
  const ints: number[] = [0];
  const v: any = 3;
  ints[0] = v;
  console.log("int slot   :", ints[0]);

  const floats: number[] = [0.5];
  const f: any = 3.5;
  floats[0] = f;
  console.log("float slot :", floats[0]);

  // a member read of an `any` is the shape that answered the raw box
  const obj: any = { x: 7 };
  const fromMember: number[] = [0];
  fromMember[0] = obj.x;
  console.log("member src :", fromMember[0]);

  // string elements take the ToString row and really own the result
  const strs: string[] = ["seed"];
  const s: any = "grown";
  strs[0] = s;
  console.log("string slot:", strs[0], strs[0].length);

  // writing past the end grows the array, and the grown slot decodes
  // the same way the in-bounds one does
  const grown: number[] = [];
  grown[0] = v;
  console.log("grown      :", grown.length, grown[0]);

  // reassigning the same slot repeatedly releases the old element
  const reused: string[] = ["a"];
  for (let i = 0; i < 4; i++) {
    reused[0] = s;
  }
  console.log("reassigned :", reused[0]);

  // an Array<Any> slot still takes the box, not a decode
  const anys: any[] = [0];
  anys[0] = v;
  console.log("any slot   :", anys[0]);

  // typed sources are untouched by the new rows
  const plain: number[] = [0];
  plain[0] = 42;
  console.log("literal    :", plain[0]);

  const bools: boolean[] = [false];
  const b: any = true;
  bools[0] = b;
  console.log("bool slot  :", bools[0]);
}

main();
