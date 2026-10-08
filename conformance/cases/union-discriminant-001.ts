// A discriminated union of object types (TS §4.24): branching on the
// shared `kind` field selects a member, whose own fields are then read.

type Shape = { kind: "circle"; r: number } | { kind: "rect"; w: number; h: string };

function area(s: Shape): string {
  if (s.kind === "circle") {
    return "circle " + s.r * 2;
  }
  return "rect " + s.w + " " + s.h;
}

function sw(s: Shape): string {
  switch (s.kind) {
    case "circle":
      return "c" + s.r;
    case "rect":
      return "r" + s.h;
  }
  return "?";
}

const shapes: Shape[] = [
  { kind: "circle", r: 2 },
  { kind: "rect", w: 3, h: "tall" },
];
for (const s of shapes) console.log(area(s), sw(s), s.kind);

type A = { kind: "a"; x: number };
type B = { kind: "b"; y: string };
function g(v: A | B): string {
  if (v.kind === "a") return "x=" + v.x;
  else return "y=" + v.y;
}
console.log(g({ kind: "a", x: 7 }), g({ kind: "b", y: "q" }));

// three members, a field name shared with different types
type Ev = { k: "a"; v: number } | { k: "b"; v: string } | { k: "c"; w: boolean };
function show(e: Ev): string {
  if (e.k === "a") {
    return "A" + (e.v + 1);
  } else if (e.k === "b") {
    return "B" + e.v.length;
  } else {
    return "C" + e.w;
  }
}
const evs: Ev[] = [{ k: "a", v: 1 }, { k: "b", v: "xyz" }, { k: "c", w: true }];
console.log(evs.map(show).join(" "));
console.log(evs.filter((e) => e.k !== "c").length);
console.log(JSON.stringify(evs));

// `in` selects a member by a field only it has
const objs: ({ t: number } | { u: string })[] = [{ t: 1 }, { u: "x" }];
for (const o of objs) {
  if ("t" in o) console.log("t", o.t);
  else console.log("u", o.u);
}

// a union of classes: instanceof selects one
class Cat {
  meow(): string {
    return "meow";
  }
}
class Dog {
  bark(): string {
    return "woof";
  }
}
function speak(a: Cat | Dog): string {
  if (a instanceof Cat) return a.meow();
  return a.bark();
}
console.log(speak(new Cat()), speak(new Dog()));
