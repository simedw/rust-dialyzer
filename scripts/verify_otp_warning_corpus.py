#!/usr/bin/env python3
"""Compare every prepared semantic case with Dialyzer from the selected OTP.

Uses the repository's compiled corpus, builds fresh version-specific library
PLTs, and runs native BEAM lifting in Rust. Target facts never enter the prior.
"""
import argparse
import collections
import fcntl
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "bench/phase0"))
from compare_warnings import expand, items

CAPTURE = r'''
Code.require_file("scripts/dependency_export.exs", File.cwd!())
[input,out] = System.argv()
File.mkdir_p!(out)
rows = File.read!(input) |> :json.decode()
build = fn name,files ->
  path = Path.join(out,name<>".plt")
  unless File.exists?(path) do
    :dialyzer.run(analysis_type: :plt_build,files: Enum.map(files,&String.to_charlist/1),
      output_plt: String.to_charlist(path),get_warnings: false,check_plt: false)
  end
  plt = :dialyzer_cplt.from_file(String.to_charlist(path))
  prior = Path.join(out,name<>".json")
  try do File.write!(prior,:json.encode(ExportDependencies.build(plt,path,true)))
  after :dialyzer_plt.delete(plt) end
  {path,prior}
end
otp_files = Enum.flat_map([:erts,:kernel,:stdlib],fn app -> :code.lib_dir(app) |> List.to_string() |> Path.join("ebin/*.beam") |> Path.wildcard() end)
otp = build.("otp-library",otp_files)
elixir_files = Path.join(:code.lib_dir(:elixir) |> List.to_string(),"ebin/*.beam") |> Path.wildcard()
elixir = if Enum.any?(rows,&(&1["kind"] != "otp")),do: build.("elixir-library",otp_files++elixir_files),else: otp
lib = :code.lib_dir(:dialyzer) |> List.to_string() |> Path.join("ebin")
prepared = Enum.map(rows,fn row ->
  directory = Path.join(out,row["slug"]); File.mkdir_p!(directory)
  {base,prior} = if row["kind"] == "otp",do: otp,else: elixir
  files = Path.wildcard(Path.join(row["beams_dir"],"*.beam"))
  if files == [],do: raise("missing corpus BEAMs")
  opts = File.read!(row["native_json"]) |> :json.decode() |> Map.fetch!("options") |> CoreExport.rebuild_terms()
  native_file = Path.join(directory,"native.json")
  warnings = :dialyzer.run([files: Enum.map(files,&String.to_charlist/1),init_plt: String.to_charlist(base),check_plt: false]++opts)
  File.write!(native_file,:json.encode(%{warnings: CoreExport.export_terms(warnings)}))
  built = :dialyzer_options.build(opts)
  options = %{id: row["id"],legal_warnings: Enum.map(elem(built,13),&Atom.to_string/1),
    purged_modules: Enum.map(files,fn path -> {:ok,{m,_}} = :beam_lib.chunks(String.to_charlist(path),[:exports]); Atom.to_string(m) end),
    use_contracts: Keyword.get(opts,:use_spec,true),solvers: Enum.map(Keyword.get(opts,:solvers,[]),&Atom.to_string/1),location: Atom.to_string(Keyword.get(opts,:error_location,:column))}
  manifest = %{beam_files: files,dialyzer_typesig_beam: Path.join(lib,"dialyzer_typesig.beam"),dialyzer_contracts_beam: Path.join(lib,"dialyzer_contracts.beam")}
  mp=Path.join(directory,"manifest.json");op=Path.join(directory,"options.json")
  File.write!(mp,:json.encode(manifest));File.write!(op,:json.encode(options))
  IO.puts(row["id"]<>" captured")
  %{id: row["id"],prior: prior,manifest: mp,options: op,native_json: native_file,out: directory}
end)
File.write!(Path.join(out,"cases.json"),:json.encode(prepared))
'''


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def warning_set(raw):
    return collections.Counter(json.dumps(w, sort_keys=True) for w in items(expand(raw) if "terms" in raw else raw))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--otp-install", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--cases", type=Path, default=ROOT / ".cache/engine/manifest.json")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--reuse-oracle", action="store_true")
    parser.add_argument("--capture-only", action="store_true")
    args = parser.parse_args()
    out = args.out.resolve(); out.mkdir(parents=True, exist_ok=True)
    binary = args.binary.resolve(); candidate = out / "analyzer"
    env = dict(os.environ, PATH=str(args.otp_install.resolve() / "bin") + os.pathsep + os.environ["PATH"], ERL_FLAGS="+S 2:2")
    major = subprocess.check_output([str(args.otp_install.resolve() / "bin/erl"), "-noshell", "-eval", 'io:format("~s",[erlang:system_info(otp_release)]),halt().'], env=env, text=True)
    if major not in ("28", "29"):
        parser.error("OTP must be 28 or 29")
    sources = [ROOT / "Cargo.toml",ROOT / "Cargo.lock",*sorted((ROOT / "src").rglob("*.rs"))]
    before = {str(p.relative_to(ROOT)): digest(p) for p in sources}
    oracle = out / "capture.exs";oracle.write_text(CAPTURE)
    with open("/tmp/rust-dialyzer-run.lock", "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        if not args.capture_only:
            shutil.copy2(binary, candidate)
            candidate_hash = digest(candidate)
        if not args.reuse_oracle:
            with (out / "capture.log").open("w") as log:
                subprocess.run(["elixir", str(oracle), str(args.cases.resolve()), str(out)],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        cases = json.loads((out / "cases.json").read_text())
        if args.capture_only:
            print(json.dumps({"otp": major,"captured": len(cases)}));return
        inputs = {args.cases.resolve(), out / "cases.json", oracle, Path(__file__).resolve()}
        for case in cases:
            for key in ("prior", "manifest", "options", "native_json"):
                inputs.add(Path(case[key]))
            manifest = json.loads(Path(case["manifest"]).read_text())
            inputs.update(map(Path,manifest["beam_files"]))
            inputs.update(Path(manifest[key]) for key in ("dialyzer_typesig_beam","dialyzer_contracts_beam"))
            prior_profile = json.loads(Path(case["prior"]).read_text())["profile"]
            if prior_profile["otp"] != major:
                raise RuntimeError("captured oracle belongs to a different OTP major")
        input_hashes = {str(path):digest(path) for path in inputs}
        results = []
        for case in cases:
            directory = Path(case["out"]);actual_path = directory / "rust.json"
            record = {"id": case["id"]}
            try:
                with actual_path.open("w") as stdout,(directory / "rust.stderr").open("w") as stderr:
                    result = subprocess.run([str(candidate),case["prior"],case["manifest"],case["options"],"--warnings-only"],cwd=ROOT,env=env,stdout=stdout,stderr=stderr,timeout=120)
                actual = json.loads(actual_path.read_text())
                record.update(status=actual.get("status"),detail=actual.get("detail"),exit_code=result.returncode,passed=False)
                if result.returncode == 0 and actual.get("status") == "ok":
                    expected = warning_set(json.loads(Path(case["native_json"]).read_text())["warnings"])
                    observed = warning_set(actual["warnings"])
                    record.update(passed=expected==observed,native_warnings=sum(expected.values()),rust_warnings=sum(observed.values()),missing=list((expected-observed).elements()),extra=list((observed-expected).elements()))
            except (subprocess.TimeoutExpired,ValueError) as error:
                record.update(passed=False,detail=str(error))
            if actual_path.exists():
                with actual_path.open("rb") as src,gzip.open(str(actual_path)+".gz","wb",compresslevel=1) as dest: shutil.copyfileobj(src,dest)
                actual_path.unlink()
            results.append(record)
            report = {"otp": major,"runtime_profile": prior_profile,"required": len(cases),"completed": len(results),"passed": sum(r["passed"] for r in results),"binary_sha256": candidate_hash,"source_sha256": before,"input_sha256":input_hashes,"cases": results}
            (out / "report.json").write_text(json.dumps(report,indent=2)+"\n")
            print(json.dumps(record),flush=True)
    if digest(candidate) != candidate_hash:
        raise RuntimeError("analyzer snapshot changed during verification")
    if before != {str(p.relative_to(ROOT)): digest(p) for p in sources}:
        raise RuntimeError("analysis sources changed during verification")
    if input_hashes != {str(path):digest(path) for path in inputs}:
        raise RuntimeError("oracle inputs changed during verification")
    if report["passed"] != len(cases): raise SystemExit(1)


if __name__ == "__main__":
    main()
