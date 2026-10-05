import sys

def smallest_factor(n):
    d = 2
    while d * d <= n:
        if n % d == 0:
            return d
        d += 1
    return n


limit = int(sys.argv[1])
total = 0
n = 2
while n < limit:
    total += smallest_factor(n)
    n += 1
print(total)
