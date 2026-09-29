import sys

def loop_sum(n, k):
    xs = []
    for i in range(n):
        xs.append(i)
    add = lambda x: x + k
    ys = list(map(add, xs))
    s = 0
    for y in ys:
        s = s + y
    return s


print(loop_sum(int(sys.argv[1]), int(sys.argv[2])))
