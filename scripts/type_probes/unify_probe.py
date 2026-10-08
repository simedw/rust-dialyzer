import json, random, subprocess
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parents[2]
root = WORKSPACE / ".cache/types"
root.mkdir(parents=True, exist_ok=True)
r = random.Random(28308)
k = lambda kind, **kw: dict(kind=kind, **kw)
none = k("none")
unit = k("unit")
anyt = k("any")
atom = k("atoms", values=["a"])
integer = k("integers", values=["1"])
opaque = k("opaque")
x = k("var", id=dict(atom="x"))
y = k("var", id=dict(integer="2"))
z = k("var", id=dict(atom="z"))
base = [
    none,
    unit,
    anyt,
    atom,
    integer,
    opaque,
    x,
    y,
    z,
    k("nil"),
    k("map_any"),
    k("tuple_any"),
]
nom = lambda n, t: k("nominal", name=dict(symbol=n), inner=t)
sets = [
    k("union", values=[nom("a", atom), integer]),
    k("union", values=[nom("a", x), integer]),
    k("union", values=[nom("a", x), nom("b", y)]),
]
types = base + [nom(n, t) for n in ["a", "b"] for t in base] + sets
for a, b in [
    (x, y),
    (x, atom),
    (atom, x),
    (integer, atom),
    (x, x),
    (y, y),
    (atom, integer),
]:
    types.extend(
        [
            k("tuple", elements=[a, b]),
            k("product", values=[a, b]),
            k("function", domain=[a], range=b),
            k("list", elements=a, tail=b, nonempty=False),
            k(
                "map",
                entries=[dict(key=atom, presence="mandatory", value=a)],
                default_key=none,
                default_value=none,
            ),
        ]
    )
types += [
    k(
        "tuple_set",
        values=[
            k("tuple", elements=[k("atoms", values=[tag]), x]) for tag in ["a", "b"]
        ],
    ),
    k(
        "tuple_set",
        values=[
            k("tuple", elements=[k("atoms", values=[tag]), integer])
            for tag in ["a", "b"]
        ],
    ),
]
types += [
    k("product", values=[]),
    k("product_raw", values=[x]),
    k("product", values=[x, y, z]),
    k("function", domain=[], range=x),
    k("function", domain=[x, y], range=z),
    k(
        "tuple_set",
        values=[k("tuple", elements=[atom, x]), k("tuple", elements=[atom, x, y])],
    ),
]
for values in [[x, y, z], [y, x, atom], [integer, atom, anyt]]:
    types += [
        k(
            "map",
            entries=[
                dict(key=k("atoms", values=[tag]), presence=presence, value=value)
                for tag, value in zip(["a", "b", "z"], values)
            ],
            default_key=none,
            default_value=none,
        )
        for presence in ["mandatory", "optional"]
    ]
commands = [dict(op="unify", a=a, b=b) for a in types for b in types]
# Independent finite pairs over the whole represented type domain.
def nested(d):
    if not d:
        return r.choice(base)
    c = r.randrange(6)
    if c == 0:
        return nom(r.choice(["a", "b"]), nested(d - 1))
    if c == 1:
        return k("tuple", elements=[nested(d - 1), nested(d - 1)])
    if c == 2:
        return k("product", values=[nested(d - 1), nested(d - 1)])
    if c == 3:
        return k("function", domain=[nested(d - 1)], range=nested(d - 1))
    if c == 4:
        return k(
            "list",
            elements=nested(d - 1),
            tail=nested(d - 1),
            nonempty=r.choice([False, True]),
        )
    return k(
        "map",
        entries=[
            dict(
                key=atom,
                presence=r.choice(["mandatory", "optional"]),
                value=nested(d - 1),
            )
        ],
        default_key=none,
        default_value=none,
    )


commands += [
    dict(op="unify", a=nested(r.randint(0, 3)), b=nested(r.randint(0, 3)))
    for _ in range(6000)
]
p = root / "unify_commands.jsonl"
out = root / "unify_results.jsonl"
p.write_text("".join(json.dumps(c) + "\n" for c in commands))
with p.open("rb") as src, out.open("wb") as dest:
    subprocess.run(
        [str(WORKSPACE / "target/release/type_probe")],
        stdin=src,
        stdout=dest,
        check=True,
    )
subprocess.run(
    ["elixir", str(WORKSPACE / "scripts/type_oracle.exs"), str(p), str(out)], check=True
)
