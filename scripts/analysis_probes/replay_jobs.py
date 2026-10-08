"""Run independent JSONL oracle groups with bounded process concurrency."""
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
import shutil
import subprocess


def replay(command, native, output, root, jobs):
    if jobs == 1:
        with native.open('rb') as stdin, output.open('wb') as stdout:
            subprocess.run(command, stdin=stdin, stdout=stdout, cwd=root, check=True)
        return
    directory = output.parent / (output.stem + '-jobs')
    directory.mkdir(parents=True, exist_ok=True)

    def run(index, source, result):
        errors = directory / (str(index) + '.stderr')
        with source.open('rb') as stdin, result.open('wb') as stdout, errors.open('wb') as stderr:
            subprocess.run(command, stdin=stdin, stdout=stdout, stderr=stderr,
                cwd=root, check=True)

    pending = set()
    count = 0
    with ThreadPoolExecutor(max_workers=jobs) as executor, native.open('rb') as rows:
        for index, line in enumerate(rows):
            while len(pending) >= jobs:
                completed, pending = wait(pending, return_when=FIRST_COMPLETED)
                for task in completed:
                    task.result()
            source = directory / (str(index) + '-input.jsonl')
            result = directory / (str(index) + '-output.jsonl')
            source.write_bytes(line)
            pending.add(executor.submit(run, index, source, result))
            count += 1
        for task in pending:
            task.result()
    with output.open('wb') as combined:
        for index in range(count):
            with (directory / (str(index) + '-output.jsonl')).open('rb') as result:
                shutil.copyfileobj(result, combined)


def positive_jobs(value):
    jobs = int(value)
    if jobs < 1:
        raise ValueError('jobs must be positive')
    return jobs
