from typing import TypeVar

T = TypeVar("T")


def id_(x: T) -> T:
    return x


def loop_sum(xs):
    s = 0
    for x in xs:
        s = s + id_(x)
    return s


def main():
    xs = []
    for i in range(10_000_000):
        xs.append(i)
    print(loop_sum(xs))


main()
