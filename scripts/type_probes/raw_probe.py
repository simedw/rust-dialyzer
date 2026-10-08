import json, itertools, subprocess
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parents[2]
root = WORKSPACE / ".cache/types"
root.mkdir(parents=True, exist_ok=True)
k = lambda kind, **kw: dict(kind=kind, **kw)
none = k("none")
anyt = k("any")
unit = k("unit")
opaque = k("opaque")
slots = [
    k("atoms", values=["a"]),
    k("bitstring", unit="8", base="0"),
    k("function", domain=[anyt], range=anyt),
    k("identifiers", values=["pid"]),
    k("proper_list", elements=anyt, nonempty=False),
    k("integers", values=["1"]),
    k("tuple", elements=[anyt]),
    k("map_any"),
]
types = [none, anyt, unit, opaque] + slots
for size in [0, 1, 2, 8]:
    for indices in itertools.combinations(range(8), size):
        types.append(
            k(
                "union_raw",
                values=[s if i in indices else none for i, s in enumerate(slots)],
            )
        )
types += [
    k("var", id={"atom": "x"}),
    k("var", id={"integer": "1"}),
    k(
        "nominal_set",
        nominals=[
            k("nominal", name=dict(symbol="a"), inner=k("var", id={"atom": "x"}))
        ],
        structural=none,
    ),
    k(
        "nominal_set",
        nominals=[
            k("nominal", name=dict(symbol="a"), inner=k("var", id={"integer": "1"}))
        ],
        structural=k("var", id={"atom": "y"}),
    ),
]
types += [k("product_raw", values=[t]) for t in [none, unit, anyt, slots[0]]]
types += [
    k("nominal_raw", name=dict(symbol="a"), inner=t)
    for t in [none, unit, anyt, types[12], types[13]]
]
types += [
    k(
        "tuple",
        elements=[
            k(
                "nominal_set",
                nominals=[
                    k(
                        "nominal",
                        name=dict(symbol="a"),
                        inner=k("var", id={"atom": "x"}),
                    )
                ],
                structural=none,
            ),
            k(
                "proper_list",
                elements=k("proper_list", elements=anyt, nonempty=False),
                nonempty=False,
            ),
        ],
    )
]
commands = [
    dict(op=op, a=x, b=y)
    for x in types
    for y in types
    for op in ["join", "meet", "subtract", "subtype"]
]
commands += [
    dict(op=op, a=x)
    for x in types
    for op in ["canonical", "structural", "elements", "has_var", "collect_var_names"]
]
commands += [
    dict(op="limit", a=x, depth=d) for x in types for d in [-1, 0, 1, 2, 3, 10]
]
p = root / "raw_commands.jsonl"
out = root / "raw_results.jsonl"
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
