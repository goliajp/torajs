import sys

def gcd(a, b):
    while b != 0:
        t = b
        b = a % b
        a = t
    return a


total = 0
limit = int(sys.argv[1])
target = int(sys.argv[2])
for i in range(1, limit + 1):
    total += gcd(i, target)

print(total)
