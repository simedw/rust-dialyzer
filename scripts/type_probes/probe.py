import json, random, subprocess
from pathlib import Path

WORKSPACE = Path(__file__).resolve().parents[2]
root = WORKSPACE / ".cache/types"
root.mkdir(parents=True, exist_ok=True)
r = random.Random(283)
types = [
    {"kind": k} for k in ["none", "any", "unit", "atom", "integer", "float", "number"]
]
for n in [0, 1, 12, 13, 14, 20]:
    types += [
        {"kind": "atoms", "values": ["a" + str(i) for i in range(n)]},
        {"kind": "integers", "values": [str(i) for i in range(n)]},
    ]
for a, b in [
    ("neg_inf", "neg_inf"),
    ("pos_inf", "pos_inf"),
    ("neg_inf", "pos_inf"),
    ("pos_inf", "neg_inf"),
    ("0", "12"),
    ("0", "13"),
    ("1", "14"),
    ("0", "255"),
    ("1", "255"),
    ("0", "256"),
    ("1", "1114111"),
    ("0", "1114112"),
    ("-14", "-1"),
    ("-1", "13"),
    ("1", "pos_inf"),
    ("0", "pos_inf"),
    ("-1", "pos_inf"),
    ("neg_inf", "-1"),
    ("neg_inf", "0"),
]:
    types.append({"kind": "range", "from": a, "to": b})
for n in [10**100, -(10**100), 2**64, -(2**64)]:
    types.extend(
        [
            {"kind": "integers", "values": [str(n), str(n + 1), str(n + 14)]},
            {"kind": "range", "from": str(n), "to": str(n + 13)},
        ]
    )
for _ in range(40):
    types.append(
        {
            "kind": "integers",
            "values": [str(r.randint(-10000, 10000)) for _ in range(r.randint(0, 25))],
        }
    )
for a in [{"kind": "atoms", "values": ["a", "b"]}, {"kind": "atom"}]:
    for n in [
        {"kind": "integers", "values": ["1", "2"]},
        {"kind": "integer"},
        {"kind": "float"},
        {"kind": "number"},
    ]:
        types.append({"kind": "union", "values": [a, n]})
commands = [
    {"op": op, "a": a, "b": b}
    for a in types
    for b in types
    for op in ["join", "meet", "subtract", "subtype"]
]
commands.extend(
    {"op": "limit", "a": a, "depth": k} for a in types for k in [-1, 0, 1, 3, 4]
)
commands.extend({"op": "canonical", "a": a} for a in types)
(root / "commands.jsonl").write_text("".join(json.dumps(c) + "\n" for c in commands))
with (root / "commands.jsonl").open("rb") as inputs, (root / "results.jsonl").open(
    "wb"
) as outputs:
    subprocess.run(
        [str(WORKSPACE / "target/release/type_probe")],
        stdin=inputs,
        stdout=outputs,
        check=True,
    )
subprocess.run(
    [
        "elixir",
        str(WORKSPACE / "scripts/type_oracle.exs"),
        str(root / "commands.jsonl"),
        str(root / "results.jsonl"),
    ],
    check=True,
)
