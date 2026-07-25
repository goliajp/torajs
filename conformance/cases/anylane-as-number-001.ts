// `x as number` over an `any` materializes the numeric face with a
// ToNumber whose result is f64. The container width analysis read the
// assertion as transparent, so a `number[]` fed by one stayed on
// integer slots and the store hit the loud "width analysis missed
// this write".

function main() {
  const obj: any = { i: 3, f: 3.5 };

  // an integer-looking source is still an f64 arrival: what the value
  // came from says nothing about how the assertion delivers it
  const pushed: number[] = [];
  pushed.push(obj.i as number);
  pushed.push(obj.f as number);
  console.log("push       :", pushed.join("|"));

  const stored: number[] = [0, 0];
  stored[0] = obj.i as number;
  stored[1] = obj.f as number;
  console.log("index store:", stored.join("|"));

  // the same crossing one frame in, through a for-of over an `any`
  const src: any = [1, 2, 3];
  const seen: number[] = [];
  for (const v of src) {
    seen.push(v as number);
  }
  console.log("for-of push:", seen.join("|"));

  // positions that already worked stay working
  const bound: number = obj.i as number;
  console.log("binding    :", bound);
  console.log("arithmetic :", (obj.i as number) + 1);

  function take(n: number) {
    return n * 2;
  }
  console.log("call arg   :", take(obj.i as number));

  // a tracked integer source keeps its integer face — the assertion is
  // transparent whenever the value underneath is already a number
  const plain: number[] = [];
  const n = 7;
  plain.push(n as number);
  console.log("tracked int:", plain.join("|"));

  // a string assertion is untouched by any of this
  const s: any = { v: "hi" };
  const text: string = s.v as string;
  console.log("as string  :", text);
}

main();
