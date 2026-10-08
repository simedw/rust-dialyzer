import json, random, subprocess
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parents[2]
root = WORKSPACE / ".cache/types"
root.mkdir(parents=True, exist_ok=True)
r = random.Random(28305)


def k(kind, **args):
    return {"kind": kind, **args}


a = k("any")
none = k("none")
unit = k("unit")
opaque = k("opaque")
atom = k("atom")
integer = k("integer")
nil = k("nil")


def name(s):
    return {"symbol": s}


names = [name(s) for s in ["alpha", "beta", "gamma"]] + [
    dict(module=m, name="t", arity=0, opacity=o)
    for m in ["mod_a", "mod_b"]
    for o in ["opaque", "transparent"]
]


def nom(n, t):
    return k("nominal", name=n, inner=t)


def union(*ts):
    return k("union", values=list(ts))


base = [
    a,
    none,
    unit,
    opaque,
    atom,
    integer,
    nil,
    k("tuple_any"),
    k("map_any"),
    k("var", id={"atom": "x"}),
    k("product", values=[a, integer]),
    k("function", domain=[integer], range=atom),
]
nominals = [nom(n, t) for n in names for t in [a, atom, integer, opaque]]
for outer in names[:3]:
    for inner in names[:3]:
        nominals.append(nom(outer, nom(inner, a)))
        nominals.append(nom(outer, nom(inner, atom)))
nominals.extend(
    [
        nom(names[0], nom(names[1], nom(names[2], a))),
        nom(names[1], nom(names[2], atom)),
        nom(names[0], nom(names[2], atom)),
    ]
)
sets = [
    union(nom(names[0], atom), nom(names[1], atom)),
    union(nom(names[0], atom), integer),
    union(nom(names[0], atom), atom),
    union(nom(names[0], atom), nom(names[1], integer)),
    union(nom(names[0], opaque), nom(names[1], atom)),
]
for n in names[:3]:
    for t in sets[:4]:
        nominals.append(nom(n, t))
types = base + nominals + sets
commands = [
    dict(op=op, a=x, b=y)
    for x in types
    for y in types
    for op in ["join", "meet", "subtract", "subtype"]
]
commands.extend(
    dict(op="limit", a=x, depth=d) for x in types for d in [-1, 0, 1, 2, 3, 4]
)
commands.extend(dict(op="canonical", a=x) for x in types)
for _ in range(8000):

    def nested(d):
        if not d:
            return r.choice(base + nominals[:28])
        choice = r.randrange(6)
        if choice == 0:
            return nom(r.choice(names), nested(d - 1))
        if choice == 1:
            return k("tuple", elements=[nested(d - 1), nested(d - 1)])
        if choice == 2:
            return k(
                "proper_list", elements=nested(d - 1), nonempty=r.choice([True, False])
            )
        if choice == 3:
            return union(nested(d - 1), nested(d - 1))
        if choice == 4:
            return k("function", domain=[nested(d - 1)], range=nested(d - 1))
        return k(
            "map",
            entries=[
                dict(
                    key=k("atoms", values=["a"]),
                    presence=r.choice(["mandatory", "optional"]),
                    value=nested(d - 1),
                )
            ],
            default_key=none,
            default_value=none,
        )

    commands.append(
        dict(
            op=r.choice(["join", "meet", "subtract", "subtype"]),
            a=nested(r.randint(0, 4)),
            b=nested(r.randint(0, 4)),
        )
    )
commands.extend(dict(op="elements", a=x) for x in types)
commands.extend(dict(op="structural", a=x) for x in types)
commands.extend(
    dict(op="is_opaque", a=x, module=module)
    for x in types
    for module in ["mod_a", "mod_b", "external"]
)
commands.extend(
    dict(op="opacity_conflict", a=x, b=y, module=module)
    for x in types[:40]
    for y in types[:40]
    for module in ["mod_a", "mod_b", "external"]
)
paths = root / "nominal_commands.jsonl"
results = root / "nominal_results.jsonl"
paths.write_text("".join(json.dumps(c) + "\n" for c in commands))
with paths.open("rb") as inputs, results.open("wb") as outputs:
    subprocess.run(
        [str(WORKSPACE / "target/release/type_probe")],
        stdin=inputs,
        stdout=outputs,
        check=True,
    )
subprocess.run(
    ["elixir", str(WORKSPACE / "scripts/type_oracle.exs"), str(paths), str(results)],
    check=True,
)
