import sys

def loop_sum(xs, f):
    s = 0
    for x in xs:
        s = s + f(x)
    return s


def main():
    xs = []
    for i in range(int(sys.argv[1])):
        xs.append(i)
    offset = int(sys.argv[2])
    delta = offset
    add = lambda x: x + delta
    print(loop_sum(xs, add))


main()
