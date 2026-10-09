"""Disk reserve for local benchmark builds and runs."""

from pathlib import Path
import shutil


MIN_FREE_BYTES = 10 * 1024 ** 3


def require_disk_reserve(path):
    directory = Path(path).resolve()
    while not directory.exists():
        directory = directory.parent
    free = shutil.disk_usage(directory).free
    if free < MIN_FREE_BYTES:
        raise RuntimeError(
            f"Benchmark stopped: {free / 1024 ** 3:.1f} GiB free at {directory}; "
            "keep at least 10 GiB free. Archive completed benchmark outputs "
            "with bench/phase0/archive_benchmark_outputs.py before continuing."
        )


if __name__ == "__main__":
    import argparse

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", type=Path)
    require_disk_reserve(parser.parse_args().path)
