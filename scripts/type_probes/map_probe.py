import json, random, subprocess
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parents[2]
root = WORKSPACE / ".cache/types"
root.mkdir(parents=True, exist_ok=True)
r = random.Random(28304)


def k(name, **args):
    return {"kind": name, **args}


a = k("any")
n = k("none")
u = k("unit")
atom = k("atom")
integer = k("integer")


def at(s):
    return k("atoms", values=[s])


def i(v):
    return k("integers", values=[str(v)])


def entry(key, value, p="mandatory"):
    return dict(key=key, presence=p, value=value)


def m(es=[], dk=n, dv=n):
    return k("map", entries=es, default_key=dk, default_value=dv)


base = [
    a,
    n,
    u,
    atom,
    integer,
    k("float"),
    k("nil"),
    k("binary"),
    k("tuple_any"),
    k("identifier"),
    k("function", domain=None, range=a),
    k("opaque"),
    k("var", id={"atom": "x"}),
]
values = [
    a,
    n,
    u,
    at("a"),
    i(1),
    k("integers", values=["1", "2"]),
    k("proper_list", elements=integer, nonempty=False),
]
maps = [k("map_any"), m()]
for key in [at("a"), k("nil"), i(0)]:
    for value in values:
        for presence in ["mandatory", "optional"]:
            maps.append(m([entry(key, value, presence)]))
for dk in [
    a,
    atom,
    integer,
    k("range", **{"from": "0", "to": "pos_inf"}),
    k("range", **{"from": "1", "to": "pos_inf"}),
    k("proper_list", elements=a, nonempty=False),
    at("a"),
    i(7),
    k("nil"),
]:
    for dv in [a, i(1), n, u]:
        maps.append(m([], dk, dv))
for value in [i(1), n, at("a")]:
    for dk in [
        a,
        atom,
        integer,
        k("range", **{"from": "1", "to": "pos_inf"}),
        k("range", **{"from": "0", "to": "pos_inf"}),
    ]:
        maps.append(
            m(
                [entry(at("a"), value, "optional"), entry(i(0), value, "optional")],
                dk,
                i(1),
            )
        )
maps.extend(
    [
        m(
            [
                entry(at("a"), k("integers", values=["1", "2"]), "optional"),
                entry(at("b"), k("integers", values=["1", "2"]), "optional"),
            ]
        ),
        m([entry(at("a"), i(1)), entry(at("b"), i(1))]),
    ]
)
maps.extend(m([], k("opaque"), dv) for dv in [a, i(1), k("opaque")])
maps.extend(
    m([entry(at("a"), dv, p)], a, k("opaque"))
    for dv in [a, k("opaque")]
    for p in ["mandatory", "optional"]
)
maps.extend(
    m([], dk, dv)
    for dk in [
        k("var", id={"atom": "x"}),
        k("tuple", elements=[k("opaque")]),
        k("proper_list", elements=k("opaque"), nonempty=False),
        k("product", values=[k("opaque"), a]),
    ]
    for dv in [a, k("opaque")]
)
types = base + maps
commands = [
    dict(op=op, a=x, b=y)
    for x in types
    for y in types
    for op in ["join", "meet", "subtract", "subtype"]
]
commands.extend(
    dict(op="limit", a=x, depth=d) for x in maps for d in [-1, 0, 1, 2, 3, 4]
)
commands.extend(dict(op="canonical", a=x) for x in maps)
for _ in range(5000):

    def nested(d):
        if not d:
            return r.choice(base[:11] + maps[:40])
        name = r.randrange(5)
        if name == 0:
            return m(
                [
                    entry(at("a"), nested(d - 1), r.choice(["mandatory", "optional"])),
                    entry(i(0), nested(d - 1), "optional"),
                ],
                r.choice([a, n, atom, integer]),
                r.choice(values),
            )
        if name == 1:
            return k("tuple", elements=[nested(d - 1), nested(d - 1)])
        if name == 2:
            return k(
                "proper_list", elements=nested(d - 1), nonempty=r.choice([True, False])
            )
        if name == 3:
            return k("function", domain=[nested(d - 1)], range=nested(d - 1))
        return k("union", values=[nested(d - 1), nested(d - 1)])

    commands.append(
        dict(
            op=r.choice(["join", "meet", "subtract", "subtype"]),
            a=nested(r.randint(0, 3)),
            b=nested(r.randint(0, 3)),
        )
    )
for x in maps + [n, u]:
    for key in [
        a,
        n,
        u,
        at("a"),
        at("b"),
        atom,
        i(0),
        i(1),
        integer,
        k("nil"),
        k("tuple_any"),
    ]:
        commands.extend(
            dict(op=op, a=x, b=key) for op in ["map_get", "map_is_key", "map_remove"]
        )
        commands.extend(
            dict(op=op, a=x, b=key, value=value)
            for op in ["map_put", "map_update"]
            for value in [a, n, u, at("a"), i(7)]
        )
for bad in [
    m([entry(at("a"), i(1))], k("opaque"), a),
    m([entry(at("a"), i(1)), entry(at("a"), i(2))]),
    m([entry(i(0), i(1)), entry(at("a"), i(1))]),
    m([entry(k("tuple", elements=[at("a")]), i(1))]),
]:
    commands.append(dict(op="canonical", a=bad))
paths = root / "map_commands.jsonl"
results = root / "map_results.jsonl"
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
