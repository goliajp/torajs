// a discriminant field whose type is several literals, a nullable
// literal or an optional one: the compare narrows by each value
type A = { k: "a" | "c"; x: number };
type B = { k: "b"; y: string };
type C = { k: "c" | null; z: boolean };
type D = { k?: "d"; w: number };

function f(u: A | B): string {
  if (u.k === "b") return u.y;
  return "x=" + u.x;
}

function g(u: B | C): string {
  if (u.k === "b") return u.y;
  return "z=" + u.z;
}

function h(u: B | D): string {
  if (u.k !== "b") return "w=" + u.w;
  return u.y;
}

function s(u: A | B): string {
  switch (u.k) {
    case "a":
    case "c":
      return "ac:" + u.x;
    default:
      return "b:" + u.y;
  }
}

type N = { x: number };
type P = { k: "p"; next: N | null; p: number };
type Q = { k: "q"; next: N | null };

function n(u: P | Q): number {
  if (u.next !== null) return u.next.x;
  return -1;
}

console.log(f({ k: "b", y: "yy" }), f({ k: "c", x: 2 }), f({ k: "a", x: 1 }));
console.log(g({ k: "b", y: "b1" }), g({ k: null, z: true }), g({ k: "c", z: false }));
console.log(h({ k: "b", y: "hb" }), h({ k: "d", w: 4 }));
console.log(s({ k: "a", x: 1 }), s({ k: "c", x: 3 }), s({ k: "b", y: "q" }));
console.log(n({ k: "p", next: { x: 9 }, p: 0 }), n({ k: "q", next: null }));
