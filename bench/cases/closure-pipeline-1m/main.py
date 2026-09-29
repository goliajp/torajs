import sys

def add1(x):
    return x + 1


def reduce(xs, f):
    s = 0
    for i in range(len(xs)):
        s = s + f(xs[i])
    return s


xs = []
for i in range(int(sys.argv[1])):
    xs.append(i)
print(reduce(xs, add1))
