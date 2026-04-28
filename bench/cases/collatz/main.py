def steps(n):
    count = 0
    while n != 1:
        if n & 1 == 0:
            n >>= 1
        else:
            n = 3 * n + 1
        count += 1
    return count


max_steps = 0
i = 1
while i <= 1_000_000:
    s = steps(i)
    if s > max_steps:
        max_steps = s
    i += 1
print(max_steps)
