import random, re, sys, binascii

random.seed(20261108)

ALPHABET = "abcABC12 _-\n"

PATTERN_ATOMS = [
    "a", "b", "c", "1", "_", " ", ".", r"\d", r"\w", r"\s", r"\D", r"\W", r"\S",
    "[ab]", "[a-c]", "[^a-c]", "[abc1]", r"[\d]", r"[^\w]",
]
QUANT = ["", "", "", "*", "+", "?", "{2}", "{1,3}", "{2,}", "*?", "+?", "??", "{1,3}?"]


def gen_atom():
    return random.choice(PATTERN_ATOMS)


def gen_piece():
    return gen_atom() + random.choice(QUANT)


def gen_group():
    inner = "".join(gen_piece() for _ in range(random.randint(1, 3)))
    if random.random() < 0.25:
        inner = "(?:" + inner + ")"
    else:
        inner = "(" + inner + ")"
    return inner + random.choice(QUANT)


def gen_pattern():
    parts = []
    for _ in range(random.randint(1, 3)):
        parts.append(gen_group() if random.random() < 0.4 else gen_piece())
    pat = "".join(parts)
    if random.random() < 0.15:
        pat = "(" + pat + "|" + gen_piece() + ")"
    if random.random() < 0.1:
        pat = "^" + pat
    if random.random() < 0.1:
        pat = pat + "$"
    return pat


def gen_text():
    return "".join(random.choice(ALPHABET) for _ in range(random.randint(0, 12)))


def nonempty_findall(rx, text):
    out = []
    pos = 0
    while pos <= len(text):
        mm = rx.search(text, pos)
        if mm is None:
            break
        if mm.start() < mm.end():
            out.append((mm.start(), mm.end()))
            pos = mm.end()
        else:
            pos = mm.start() + 1
    return out


def hexs(s):
    return binascii.hexlify(s.encode("utf-8")).decode("ascii")


skipped = 0
for _ in range(4000):
    pat = gen_pattern()
    text = gen_text()
    try:
        rx = re.compile(pat)
    except re.error:
        skipped += 1
        continue
    m = rx.search(text)
    hs = "-" if m is None else f"{m.start()},{m.end()}"
    allm = nonempty_findall(rx, text)
    tot = sum(e for _, e in allm)
    print(f"{hexs(pat)}\t{hexs(text)}\t{hs}\t{allm}\t{tot}")

print(f"# skipped={skipped}", file=sys.stderr)
