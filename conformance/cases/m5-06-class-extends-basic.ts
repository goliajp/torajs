class Animal {
  name: string;
  constructor(n: string) { this.name = n; }
  greet(): void { console.log(this.name); }
}
class Dog extends Animal {
  bark_count: number;
  constructor(n: string, b: number) {
    super(n);
    this.bark_count = b;
  }
  bark(): void { console.log(this.bark_count); }
}
let d = new Dog("rex", 3);
d.greet();
d.bark();
