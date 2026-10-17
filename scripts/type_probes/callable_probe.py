import json, random, subprocess
from pathlib import Path
from probe_config import configuration

WORKSPACE = Path(__file__).resolve().parents[2]
CONFIG = configuration(WORKSPACE)
root = CONFIG.out
root.mkdir(parents=True, exist_ok=True)
r = random.Random(28303)


def k(name, **args):
    return {"kind": name, **args}


anyt = k("any")
none = k("none")
unit = k("unit")
opaque = k("opaque")
a = k("atoms", values=["a"])
integer = k("integer")


def f(domain=None, range=anyt):
    return k("function", domain=domain, range=range)


def bit(u, b):
    return k("bitstring", unit=str(u), base=str(b))


types = [
    anyt,
    none,
    unit,
    a,
    integer,
    k("float"),
    k("binary"),
    k("tuple_any"),
    k("nil"),
    k("identifier"),
    opaque,
    k("var", id={"atom": "x"}),
    k("var", id={"atom": "y"}),
    k("var", id={"integer": "123456789123456789123456789"}),
]
for u in [0, 1, 2, 3, 7, 8, 16, 85]:
    for b in [0, 1, 8, 16, 73]:
        types.append(bit(u, b))
for u, b in [
    (6442450944, 1456),
    (85, 7),
    (547608330240, 347892352432),
    (10**100, 10**101 + 3),
]:
    types.append(bit(u, b))
for domain in [
    None,
    [],
    [anyt],
    [integer],
    [a],
    [integer, a],
    [none],
    [unit],
    [opaque],
    [k("var", id={"atom": "x"})],
]:
    for result_range in [anyt, none, unit, a, opaque]:
        types.append(f(domain, result_range))
for es in [
    [],
    [anyt],
    [none],
    [unit],
    [integer, a],
    [anyt, anyt],
    [opaque, a],
    [k("var", id={"atom": "x"}), integer],
]:
    types.append(k("product", values=es))
commands = [
    dict(op=op, a=x, b=y)
    for x in types
    for y in types
    for op in ["join", "meet", "subtract", "subtype", "bitstr_concat", "bitstr_match"]
]
commands.extend(
    dict(op="limit", a=x, depth=d) for x in types for d in [-1, 0, 1, 2, 3, 4]
)
commands.extend(dict(op="canonical", a=x) for x in types)
for _ in range(4000):

    def nested(d):
        if not d:
            return r.choice(types[:14])
        name = r.randrange(6)
        if name == 0:
            return f([nested(d - 1)], nested(d - 1))
        if name == 1:
            return k("tuple", elements=[nested(d - 1), nested(d - 1)])
        if name == 2:
            return k(
                "proper_list", elements=nested(d - 1), nonempty=r.choice([True, False])
            )
        if name == 3:
            return k("product", values=[nested(d - 1), nested(d - 1)])
        if name == 4:
            return k("union", values=[nested(d - 1), nested(d - 1)])
        return bit(r.randint(0, 40), r.randint(0, 400))

    commands.append(
        dict(
            op=r.choice(
                ["join", "meet", "subtract", "subtype", "bitstr_concat", "bitstr_match"]
            ),
            a=nested(r.randint(0, 3)),
            b=nested(r.randint(0, 3)),
        )
    )
commands_path = root / "callable_commands.jsonl"
results = root / "callable_results.jsonl"
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
