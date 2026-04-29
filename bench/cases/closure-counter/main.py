def loop_sum(xs, f):
    s = 0
    for x in xs:
        s = s + f(x)
    return s


def main():
    xs = []
    for i in range(10_000_000):
        xs.append(i)
    offset = 2
    add = lambda x: x + offset
    print(loop_sum(xs, add))


main()
