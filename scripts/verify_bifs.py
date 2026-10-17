import hashlib
import json
import subprocess
import argparse
import os
from pathlib import Path
from itertools import product

WORKSPACE = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description="Compare Rust BIF transfers with the selected OTP oracle.")
parser.add_argument("--otp", choices=["28", "29"], default="28")
parser.add_argument("--otp-install", type=Path)
parser.add_argument("--binary", type=Path, help="Use an existing probe instead of building one.")
parser.add_argument("--out", type=Path)
config = parser.parse_args()
ROOT = (config.out or WORKSPACE / (".cache/bifs" if config.otp == "28" else ".cache/bifs-otp29")).resolve()
ROOT.mkdir(parents=True, exist_ok=True)
if config.otp_install:
    os.environ["PATH"] = str(config.otp_install.resolve() / "bin") + os.pathsep + os.environ["PATH"]
os.environ.setdefault("ERL_FLAGS", "+S 2:2")
actual = subprocess.check_output(["erl", "-noshell", "-eval", 'io:format("~s",[erlang:system_info(otp_release)]),halt().'], text=True).strip()
if actual != config.otp:
    parser.error(f"requested OTP {config.otp}, but erl on PATH reports {actual}")
CATALOGUE = WORKSPACE / ("corpus/bifs.json" if config.otp == "28" else "corpus/bifs-otp29.json")
proof_files = [
        WORKSPACE / "src/lattice.rs",
        WORKSPACE / "src/type_api.rs",
        WORKSPACE / "src/native_order.rs",
        WORKSPACE / "src/runtime_term.rs",
        WORKSPACE / "src/presentation.rs",
        WORKSPACE / "src/presentation_form.rs",
        WORKSPACE / "Cargo.toml",
        WORKSPACE / "Cargo.lock",
        WORKSPACE / "src/native_error.rs",
        *sorted((WORKSPACE / "src/bifs").glob("*.rs")),
        WORKSPACE / "src/bin/bif_probe.rs",
        CATALOGUE,
        *sorted((WORKSPACE / "src/lattice").glob("*.rs")),
        WORKSPACE / "scripts/native_types.exs",
        WORKSPACE / "scripts/bif_oracle.exs",
        Path(__file__).resolve(),
    ]
initial_hashes = {str(p.relative_to(WORKSPACE)): hashlib.sha256(p.read_bytes()).hexdigest() for p in proof_files}
if not config.binary:
    package = "rust-dialyzer-poc" if config.otp == "28" else "rust-dialyzer-otp29"
    target = "bif_probe" if config.otp == "28" else "bif_probe_otp29"
    subprocess.run(["cargo", "build", "--release", "--offline", "-p", package, "--bin", target], cwd=WORKSPACE, check=True)
ORACLE = WORKSPACE / "scripts/bif_oracle.exs"
BINARY = config.binary.resolve() if config.binary else WORKSPACE / "target/release" / ("bif_probe" if config.otp == "28" else "bif_probe_otp29")
ctor = lambda name, *args: dict(constructor=name, arguments=list(args))
atom = lambda x: dict(atom=x)
integer = lambda x: dict(integer=str(x))
I = ctor("t_integer")
A = ctor("t_atom")
N = ctor("t_none")
numeric = (
    [I, ctor("t_number"), ctor("t_float")]
    + [ctor("t_integer", integer(n)) for n in [-3, -1, 0, 1, 2, 7]]
    + [
        ctor("t_integers", dict(list=[integer(1), integer(3)])),
        ctor("t_from_range", integer(0), integer(255)),
        ctor("t_pos_integer"),
        ctor("t_neg_integer"),
    ]
)
pool = numeric + [
    A,
    ctor("t_atom", atom("true")),
    ctor("t_atom", atom("false")),
    ctor("t_atom", atom("a")),
    ctor("t_nil"),
    ctor("t_list", I),
    ctor("t_nonempty_list", I),
    ctor("t_cons", I, ctor("t_atom", atom("a"))),
    ctor("t_tuple", dict(list=[])),
    ctor("t_tuple", dict(list=[ctor("t_atom", atom("a")), I])),
    ctor("t_tuple"),
    ctor("t_binary"),
    ctor("t_bitstr", integer(3), integer(1)),
    ctor("t_pid"),
    ctor("t_reference"),
    ctor("t_fun", dict(list=[I]), A),
    ctor("t_any"),
    N,
    ctor("t_unit"),
]
pair = lambda key, presence, value: dict(tuple=[key, atom(presence), value])
mapctor = lambda pairs, key=N, value=N: ctor("t_map", dict(list=pairs), key, value)
maps = [
    ctor("t_map"),
    mapctor([]),
    mapctor([pair(ctor("t_atom", atom("a")), "mandatory", I)]),
    mapctor([pair(ctor("t_atom", atom("a")), "optional", A)]),
    mapctor([], A, I),
    mapctor([pair(ctor("t_integer", integer(1)), "mandatory", ctor("t_binary"))]),
]
pool += maps
pool += [
    ctor("t_list", ctor("t_list", I)),
    ctor("t_nonempty_list", ctor("t_list", A)),
    ctor("t_list", ctor("t_tuple", dict(list=[I, A]))),
    ctor("t_nonempty_list", ctor("t_tuple", dict(list=[I, A, I]))),
]
callbacks = [
    ctor("t_fun", dict(list=[domain] * arity), result)
    for arity in [0, 1, 2, 3]
    for domain in [I, A]
    for result in [
        I,
        A,
        ctor("t_atom", atom("true")),
        ctor("t_atom", atom("false")),
        N,
        ctor("t_unit"),
        ctor("t_list", I),
        ctor("t_tuple", dict(list=[I, A])),
    ]
]
pool += callbacks
nominal_bases = [
    ctor("t_atom", atom("a")),
    ctor("t_integer", integer(1)),
    ctor("t_list", I),
    ctor("t_tuple", dict(list=[ctor("t_atom", atom("a")), I])),
    maps[2],
    ctor("t_fun", dict(list=[I]), I),
    ctor("t_binary"),
]
nominals = [
    ctor(
        "t_nominal",
        dict(tuple=[atom("sample"), atom("name"), integer(0), atom(opacity)]),
        base,
    )
    for opacity in ["transparent", "opaque"]
    for base in nominal_bases
]
nominals += [
    ctor(
        "t_sup",
        dict(
            list=[
                ctor("t_nominal", atom("left"), base),
                ctor("t_nominal", atom("right"), base),
            ]
        ),
    )
    for base in nominal_bases
]
pool += nominals
nominal_callbacks = [
    ctor(
        "t_fun",
        dict(list=[I] * arity),
        ctor(
            "t_nominal",
            dict(tuple=[atom("sample"), atom("range"), integer(0), atom(opacity)]),
            value,
        ),
    )
    for arity in [1, 2]
    for opacity in ["transparent", "opaque"]
    for value in [
        ctor("t_atom", atom("true")),
        ctor("t_atom", atom("false")),
        I,
        ctor("t_tuple", dict(list=[I, A])),
    ]
]
callbacks += nominal_callbacks
pool += nominal_callbacks
pool += [
    ctor(
        "t_tuple",
        dict(list=[ctor("t_nominal", atom("tag"), ctor("t_atom", atom("a"))), I]),
    ),
    ctor("t_list", ctor("t_nominal", atom("pair"), ctor("t_tuple", dict(list=[I, A])))),
]

if config.otp == "29":
    records = [ctor("t_record"), ctor("t_record", dict(tuple=[atom("m"), atom("r")]))]
    for name in ["r", "s"]:
        for value in [I, A, ctor("t_any"), ctor("t_var", atom("field"))]:
            records.append(ctor("t_record", dict(tuple=[atom("m"), atom(name)]),
                                dict(list=[dict(tuple=[atom("a"), value])])) )
    records.append(ctor("t_sup", dict(list=records[1:3] + [ctor("t_record", dict(tuple=[atom("m"), atom("s")]))])))
    pool += records
    pool += [ctor("t_sup", dict(list=[r, base])) for r in records[:3] for base in [I, A, ctor("t_tuple")]]

catalogue = json.loads(CATALOGUE.read_text())
supported = json.loads(subprocess.check_output([BINARY, "--supported"], text=True))
entries = {
    (e["mfa"]["module"], e["mfa"]["function"], e["mfa"]["arity"]): e
    for e in catalogue["entries"]
}
rows = []


def add(mfa, args, kind):
    rows.append(
        dict(
            id=str(len(rows)), kind=kind, operation="transfer", mfa=mfa, arguments=args
        )
    )


for mfa in supported:
    entry = entries[(mfa["module"], mfa["function"], mfa["arity"])]
    baseline = [
        dict(type_etf=x["etf"]) for x in entry["admissible_domains"]["arguments"]
    ]
    add(mfa, baseline, "domain_baseline")
    for constructor in ["t_any", "t_none", "t_unit"]:
        add(mfa, [ctor(constructor)] * mfa["arity"], "universal")
    for position in range(mfa["arity"]):
        for value in pool:
            args = baseline.copy()
            args[position] = value
            add(mfa, args, "single_argument")
    if (
        mfa["module"] == "erlang"
        and mfa["function"]
        in [
            "+",
            "-",
            "*",
            "div",
            "rem",
            "band",
            "bor",
            "bxor",
            "bsl",
            "bsr",
            "==",
            "/=",
            "=:=",
            "=/=",
            "<",
            ">",
            "=<",
            ">=",
        ]
        and mfa["arity"] == 2
    ):
        for args in product(numeric, repeat=2):
            add(mfa, list(args), "numeric_grid")
for name in [
    "all",
    "any",
    "filter",
    "flatmap",
    "foreach",
    "map",
    "partition",
    "sort",
    "usort",
    "foldl",
    "foldr",
    "mapfoldl",
    "mapfoldr",
    "dropwhile",
    "takewhile",
    "splitwith",
]:
    arity = 3 if name in ["foldl", "foldr", "mapfoldl", "mapfoldr"] else 2
    for fun, list_value in product(
        callbacks,
        [
            ctor("t_nil"),
            ctor("t_list", I),
            ctor("t_nonempty_list", I),
            ctor("t_list", A),
            ctor("t_nonempty_list", A),
        ],
    ):
        add(
            dict(module="lists", function=name, arity=arity),
            [fun, I, list_value] if arity == 3 else [fun, list_value],
            "callback_grid",
        )
selectors = [v["selector"] for v in catalogue["system_info_selector_returns"]]
for name in ["system_info", "info"]:
    for value in [ctor("t_atom", atom(key)) for key in selectors] + [
        ctor("t_atoms", dict(list=[atom("wordsize"), atom("endian")])),
        ctor("t_atoms", dict(list=[atom("not_known"), atom("wordsize")])),
    ]:
        add(dict(module="erlang", function=name, arity=1), [value], "system_selector")
for left, right in product(maps, repeat=2):
    add(dict(module="maps", function="merge", arity=2), [left, right], "map_grid")
for args in [[ctor("t_any")], [N], [ctor("t_unit")]]:
    add(dict(module="math", function="sqrt", arity=1), args, "unknown_fallback")
for arity in [1, 2]:
    for value in [ctor("t_any"), N, ctor("t_unit")]:
        add(
            dict(module="erlang", function="get_module_info", arity=arity),
            [value] * arity,
            "runtime_reflection",
        )
(ROOT / "native_requests.jsonl").write_text("".join(json.dumps(x) + "\n" for x in rows))
with (ROOT / "native_requests.jsonl").open() as inp, (
    ROOT / "native_responses.jsonl"
).open("w") as out:
    subprocess.run(["elixir", str(ORACLE)], stdin=inp, stdout=out, check=True, cwd=ROOT)
reference = [
    json.loads(x) for x in (ROOT / "native_responses.jsonl").read_text().splitlines()
]
if len(reference) != len(rows):
    raise RuntimeError("native result count differs")
fixtures = []
native_errors = []
for req, res in zip(rows, reference):
    if not res["ok"]:
        native_errors.append(dict(request=req, error=res["native_error"]))
        fixtures.append(
            dict(
                id=req["id"],
                mfa=req["mfa"],
                arguments=res["arguments"],
                expected_error=res["native_error"],
            )
        )
        continue
    fixtures.append(
        dict(
            id=req["id"],
            mfa=req["mfa"],
            arguments=[x["native"] for x in res["result"]["arguments"]],
            expected=res["result"]["return"]["native"],
        )
    )
(ROOT / "fixtures.jsonl").write_text("".join(json.dumps(x) + "\n" for x in fixtures))
with (ROOT / "fixtures.jsonl").open() as inp, (ROOT / "rust_responses.jsonl").open(
    "w"
) as out:
    subprocess.run([BINARY], stdin=inp, stdout=out, check=True)
results = [
    json.loads(x) for x in (ROOT / "rust_responses.jsonl").read_text().splitlines()
]
if len(results) != len(fixtures):
    raise RuntimeError("Rust result count differs")
failures = [
    dict(fixture=f, response=r)
    for f, r in zip(fixtures, results)
    if not r.get("parity")
]
(ROOT / "failures.json").write_text(json.dumps(failures, indent=2))
(ROOT / "native_errors.json").write_text(json.dumps(native_errors, indent=2))
summary = dict(
    otp=config.otp,
    binary_sha256=hashlib.sha256(BINARY.read_bytes()).hexdigest(),
    supported_mfas=len(supported),
    requested=len(rows),
    native_error_count=len(native_errors),
    compared=len(fixtures),
    passed=len(fixtures) - len(failures),
    failed=len(failures),
)
summary.update(
    differential_complete=len(supported) == len(catalogue["entries"])
    and not failures
    and len(results) == len(rows),
    native_errors_verified=True,
    required_mfas=len(catalogue["entries"]),
)
if initial_hashes != {str(p.relative_to(WORKSPACE)): hashlib.sha256(p.read_bytes()).hexdigest() for p in proof_files}:
    raise RuntimeError("BIF implementation changed during verification")
summary["implementation_sha256"] = initial_hashes
(ROOT / "report.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary))
for fail in failures[:8]:
    print(json.dumps(fail))

if failures:
    raise SystemExit(1)
