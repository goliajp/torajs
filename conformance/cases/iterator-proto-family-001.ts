// §23.1.5.2 / §22.1.5.1 / §24.1.5.2 / §24.2.5.2 / §27.1.5 — each
// iterator family has its OWN prototype, sitting between the iterator
// and %Iterator.prototype%. tr used to hang all of them straight off
// %Iterator.prototype%, a chain one link short — and a chain one link
// short has nowhere to keep the `@@toStringTag` that names the badge,
// so every iterator answered "[object Object]".
const arr: any = [1, 2]
const map: any = new Map([[1, 2]])
const set: any = new Set([1])
const str: any = "ab"

const shapes: any[] = [
  ["array @@iterator", arr[Symbol.iterator]()],
  ["array values", arr.values()],
  ["array keys", arr.keys()],
  ["array entries", arr.entries()],
  ["typed array", new Int8Array(2)[Symbol.iterator]()],
  ["map @@iterator", map[Symbol.iterator]()],
  ["map keys", map.keys()],
  ["map values", map.values()],
  ["set @@iterator", set[Symbol.iterator]()],
  ["set entries", set.entries()],
  ["string", str[Symbol.iterator]()],
  ["helper", arr.values().map((x: number) => x)],
]
for (const [name, it] of shapes) {
  console.log(name, "|", Object.prototype.toString.call(it), "|", it[Symbol.toStringTag])
}

// The badge is a real property on a real object, not a synthesized
// answer — so the two faces of it agree, and it survives nothing.
const ap: any = Object.getPrototypeOf(arr.values())
const mp: any = Object.getPrototypeOf(map.keys())
console.log("distinct per family:", ap !== mp, ap === Object.getPrototypeOf([3].values()))
console.log("own names:", JSON.stringify(Object.getOwnPropertyNames(ap)))
console.log("own symbols:", Object.getOwnPropertySymbols(ap).map((s: any) => String(s)).join(","))
const d: any = Object.getOwnPropertyDescriptor(ap, Symbol.toStringTag)
console.log("descriptor:", d.value, d.writable, d.enumerable, d.configurable)

// One link below %Iterator.prototype%, which is one link below the root.
console.log("parent:", Object.getPrototypeOf(ap) === (Iterator as any).prototype)
console.log("grandparent:", Object.getPrototypeOf(Object.getPrototypeOf(ap)) === Object.prototype)

// `next` is the family prototype's own; the lazy helpers are inherited
// from %Iterator.prototype% one link out.
console.log("owns next:", Object.prototype.hasOwnProperty.call(ap, "next"),
            "| owns map:", Object.prototype.hasOwnProperty.call(ap, "map"),
            "| reaches map:", typeof ap.map)

// No clause gives these prototypes a constructor of their own; the one
// they answer with is %Iterator.prototype%'s.
console.log("own constructor:", Object.prototype.hasOwnProperty.call(ap, "constructor"),
            "| inherited:", ap.constructor === Iterator)

// Iteration itself is unchanged.
console.log("iterates:", [...arr].join(), [...map.keys()].join(), [...set].join(), [...str].join())
