import sys


def divisor_sum(n):
    s = 0
    d = 1
    while d * d <= n:
        if n % d == 0:
            s += d
        d += 1
    return s


limit = int(sys.argv[1])
total = 0
n = 1
while n <= limit:
    total += divisor_sum(n)
    n += 1
print(total)
