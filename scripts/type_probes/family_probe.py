import json, random, subprocess
from pathlib import Path
from probe_config import configuration

WORKSPACE = Path(__file__).resolve().parents[2]
CONFIG = configuration(WORKSPACE)
root = CONFIG.out
root.mkdir(parents=True, exist_ok=True)
r = random.Random(28302)


def k(name, **args):
    return {"kind": name, **args}


anyt = k("any")
nil = k("nil")
integer = k("integer")
atom = k("atom")
a = k("atoms", values=["a"])
b = k("atoms", values=["b"])


def tup(*es):
    return k("tuple", elements=list(es))


def lst(es=anyt, tail=nil, nonempty=False):
    return k("list", elements=es, tail=tail, nonempty=nonempty)


def union(*es):
    return k("union", values=list(es))


types = [
    anyt,
    k("none"),
    k("unit"),
    integer,
    atom,
    a,
    b,
    k("float"),
    k("number"),
    nil,
    k("identifier"),
]
for ids in [
    [],
    ["pid"],
    ["port"],
    ["reference"],
    ["pid", "port"],
    ["pid", "reference"],
    ["port", "reference"],
    ["pid", "port", "reference"],
]:
    types.append(k("identifiers", values=ids))
for es in [anyt, integer, a, b, k("none"), k("unit"), nil]:
    for nonempty in [False, True]:
        types.append(lst(es, nil, nonempty))
for tail in [
    anyt,
    a,
    union(nil, a),
    k("identifiers", values=["pid"]),
    lst(integer),
    union(lst(integer), a),
    k("none"),
    k("unit"),
]:
    for nonempty in [False, True]:
        types.append(lst(integer, tail, nonempty))
for es in [
    [],
    [anyt],
    [a],
    [a, integer],
    [b, atom],
    [atom, integer],
    [integer, atom],
    [k("none"), anyt],
    [anyt, k("unit")],
    [lst(integer), tup(a, integer)],
]:
    types.append(k("tuple", elements=es))
types.append(k("tuple_any"))
for arity in [0, 1, 2, 3, 8, 9]:
    types.append(k("tuple_arity", arity=arity))
for tags in [1, 2, 5, 6, 13, 14]:
    types.append(tup(k("atoms", values=["tag" + str(i) for i in range(tags)]), integer))
for count in [5, 6, 8, 9]:
    types.append(
        k("tuple_set", values=[k("tuple", elements=[a] * n) for n in range(count)])
    )
for n in [2, 5, 6]:
    types.append(
        k(
            "tuple_set",
            values=[
                tup(k("atoms", values=["t" + str(i)]), k("integers", values=[str(i)]))
                for i in range(n)
            ],
        )
    )
for n in [1, 2, 3, 4, 5, 6]:
    x = integer
    for _ in range(n):
        x = tup(a, lst(x))
    types.append(x)
commands = [
    dict(op=op, a=x, b=y)
    for x in types
    for y in types
    for op in ["join", "meet", "subtract", "subtype"]
]
commands.extend(
    dict(op="limit", a=x, depth=d) for x in types for d in [-1, 0, 1, 2, 3, 4, 5, 6]
)
commands.extend(dict(op="canonical", a=x) for x in types)
for _ in range(6000):

    def nested(depth):
        if not depth:
            return r.choice(types[:19])
        kind = r.randrange(4)
        if kind == 0:
            return tup(nested(depth - 1), nested(depth - 1))
        if kind == 1:
            return lst(
                nested(depth - 1), union(nil, r.choice([a, b])), r.choice([False, True])
            )
        if kind == 2:
            return k("cons", head=nested(depth - 1), tail=nested(depth - 1))
        return union(nested(depth - 1), nested(depth - 1))

    x, y = nested(r.randint(0, 3)), nested(r.randint(0, 3))
    commands.append(
        dict(op=r.choice(["join", "meet", "subtract", "subtype"]), a=x, b=y)
    )
for arity in [1024, 1025]:
    for depth in [0, 1, 2]:
        commands.append(dict(op="limit", a=k("tuple_arity", arity=arity), depth=depth))
    commands.append(dict(op="canonical", a=k("tuple_arity", arity=arity)))
commands_path = root / "family_commands.jsonl"
results = root / "family_results.jsonl"
commands_path.write_text("".join(json.dumps(c) + "\n" for c in commands))
with commands_path.open("rb") as inputs, results.open("wb") as outputs:
    subprocess.run(
        [str(CONFIG.binary)],
        stdin=inputs,
        stdout=outputs,
        check=True,
    )
subprocess.run(
    [
        "elixir",
        str(WORKSPACE / "scripts/type_oracle.exs"),
        str(commands_path),
        str(results),
    ],
    check=True,
)
