import json, random, subprocess
from pathlib import Path
from probe_config import configuration

WORKSPACE = Path(__file__).resolve().parents[2]
CONFIG = configuration(WORKSPACE)
root = CONFIG.out
root.mkdir(parents=True, exist_ok=True)
r = random.Random(28307)
k = lambda kind, **kw: dict(kind=kind, **kw)
none = k("none")
unit = k("unit")
anyt = k("any")
atom = k("atoms", values=["a"])
integer = k("integers", values=["1"])
opaque = k("opaque")
x = k("var", id=dict(atom="x"))
y = k("var", id=dict(integer="2"))
base = [
    none,
    unit,
    anyt,
    atom,
    integer,
    opaque,
    x,
    y,
    k("nil"),
    k("map_any"),
    k("tuple_any"),
]
sets = (
    [[]]
    + [[dict(id=x["id"], type=t)] for t in base]
    + [[dict(id=x["id"], type=a), dict(id=y["id"], type=b)] for a in base for b in base]
)


def nested(d):
    if not d:
        return r.choice(base)
    choice = r.randrange(7)
    if choice == 0:
        return k("nominal", name=dict(symbol=r.choice(["a", "b"])), inner=nested(d - 1))
    if choice == 1:
        return k("tuple", elements=[nested(d - 1), nested(d - 1)])
    if choice == 2:
        return k(
            "list",
            elements=nested(d - 1),
            tail=nested(d - 1),
            nonempty=r.choice([True, False]),
        )
    if choice == 3:
        return k("union", values=[nested(d - 1), nested(d - 1)])
    if choice == 4:
        return k("function", domain=[nested(d - 1)], range=nested(d - 1))
    if choice == 5:
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
    return k("product", values=[nested(d - 1), nested(d - 1)])


types = base + [
    k("tuple", elements=[x, y]),
    k("list", elements=x, tail=y, nonempty=False),
    k(
        "nominal_set",
        nominals=[k("nominal", name=dict(symbol="a"), inner=x)],
        structural=none,
    ),
    k(
        "nominal_set",
        nominals=[k("nominal", name=dict(symbol="a"), inner=x)],
        structural=y,
    ),
    k(
        "union_raw",
        values=[
            none,
            none,
            k("function", domain=[x], range=y),
            none,
            none,
            none,
            none,
            none,
        ],
    ),
]
if CONFIG.otp == "29":
    types[-1]["values"].append(none)
commands = [dict(op="substitute", a=t, bindings=s) for t in types for s in sets]
commands += [
    dict(op="substitute", a=nested(r.randint(1, 4)), bindings=r.choice(sets))
    for _ in range(10000)
]
p = root / "subst_commands.jsonl"
out = root / "subst_results.jsonl"
p.write_text("".join(json.dumps(c) + "\n" for c in commands))
with p.open("rb") as src, out.open("wb") as dest:
    subprocess.run(
        [str(CONFIG.binary)],
        stdin=src,
        stdout=dest,
        check=True,
    )
subprocess.run(
    ["elixir", str(WORKSPACE / "scripts/type_oracle.exs"), str(p), str(out)], check=True
)
