// JSON serializer demo — uses torajs's JSON.stringify on user-shaped
// classes, primitive arrays, and round-trips a small parse → stringify
// chain. The pretty-printed (3-arg) form is on the roadmap for a
// follow-up phase; this example stays on the compact 1-arg form.
//
// Exercises: structural object types, JSON.stringify (compact),
// JSON.parse + caller-driven type inference, class instances as JSON
// sources.

class User {
  name: string;
  age: number;
  active: boolean;
  tags: string[];
  constructor(name: string, age: number, active: boolean, tags: string[]) {
    this.name = name;
    this.age = age;
    this.active = active;
    this.tags = tags;
  }
}

function main(): void {
  const alice = new User("Alice", 30, true, ["admin", "engineer"]);
  const bob = new User("Bob", 25, false, ["intern"]);

  console.log(JSON.stringify(alice));
  console.log(JSON.stringify(bob));

  const numbers: number[] = [1, 2, 3, 5, 8, 13, 21];
  console.log(JSON.stringify(numbers));

  const words: string[] = ["alpha", "beta", "gamma"];
  console.log(JSON.stringify(words));

  // Round-trip: parse a literal back into a typed shape, re-emit.
  const text = '[10, 20, 30]';
  const arr: number[] = JSON.parse(text);
  console.log(JSON.stringify(arr));
}

main();
