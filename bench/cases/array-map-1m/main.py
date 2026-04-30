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


print(loop_sum(10_000_000, 2))
