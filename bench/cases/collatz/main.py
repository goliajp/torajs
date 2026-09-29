import sys

def steps(n):
    count = 0
    while n != 1:
        if n & 1 == 0:
            n >>= 1
        else:
            n = 3 * n + 1
        count += 1
    return count


limit = int(sys.argv[1])
max_steps = 0
i = 1
while i <= limit:
    s = steps(i)
    if s > max_steps:
        max_steps = s
    i += 1
print(max_steps)
