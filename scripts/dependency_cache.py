"""Reuse dependency exports only while their PLT and BEAM inputs match."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def current(artifact, plt):
    try:
        return (
            artifact["version"] == 3
            and artifact["source"]["exporter_sha256"] == digest(ROOT / "scripts/export_dependencies.exs")
            and isinstance(artifact["callback_lists"], list)
            and Path(artifact["source"]["path"]).resolve() == Path(plt).resolve()
            and digest(plt) == artifact["source"]["sha256"]
            and all(
                digest(f["path"]) == f["sha256"] for f in artifact["source"]["files"]
            )
        )
    except (KeyError, OSError):
        return False


def export(plt, destination):
    destination = Path(destination)
    try:
        artifact = json.loads(destination.read_text())
    except (OSError, ValueError):
        artifact = {}
    if current(artifact, plt):
        return "reused"
    subprocess.run(
        ["elixir", "scripts/export_dependencies.exs", str(plt), str(destination)],
        cwd=ROOT,
        check=True,
    )
    if not current(json.loads(destination.read_text()), plt):
        raise RuntimeError("dependency source changed during export")
    return "exported"
