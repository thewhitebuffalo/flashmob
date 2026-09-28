#!/usr/bin/env python3
"""Compare two release binaries on identical studies, checking every result field.

Usage: python3 scripts/benchmark_studies.py /path/to/before /path/to/after
Times are median engine elapsed_ms (exclude process startup and JSON output).
Synthetic cases are balanced binary feeders, with or without cross-ties.
"""
import argparse
import copy
import json
import math
import statistics
import subprocess


def invoke(binary, project, study):
    result = subprocess.run(
        [binary, "run", "-", "--study", study, "--compact"],
        input=json.dumps(project), text=True, capture_output=True, check=True,
    )
    return json.loads(result.stdout)["results"]


def compare(a, b, path="results"):
    if isinstance(a, dict):
        assert isinstance(b, dict) and a.keys() == b.keys(), path
        for key in a:
            if key != "elapsed_ms":
                compare(a[key], b[key], f"{path}.{key}")
    elif isinstance(a, list):
        assert isinstance(b, list) and len(a) == len(b), path
        for i, (x, y) in enumerate(zip(a, b)):
            compare(x, y, f"{path}[{i}]")
    elif isinstance(a, (float, int)) and not isinstance(a, bool):
        assert isinstance(b, (float, int)) and not isinstance(b, bool), path
        assert math.isclose(a, b, rel_tol=1e-8, abs_tol=1e-7), (path, a, b)
    else:
        assert a == b, (path, a, b)


def feeder(sample, n, meshed):
    project = copy.deepcopy(sample)
    for key in ["branches", "buses", "loads", "motors", "devices", "equipment"]:
        project[key] = []
    for i in range(n):
        bus = copy.deepcopy(sample["buses"][0])
        bus.update(id=f"b{i}", name=f"Bus {i}", kv=12.47)
        project["buses"].append(bus)
        if i:
            project["loads"].append(dict(id=f"load{i}", name=f"Load {i}",
                                        bus=f"b{i}", kw=10.0, kvar=3.0, basis=""))
    project["sources"][0]["bus"] = "b0"
    edges = [((i - 1) // 2, i) for i in range(1, n)]
    if meshed:
        edges.extend((i, i + 1) for i in range(n // 2, n - 1, 3))
    for i, (f, t) in enumerate(edges):
        branch = copy.deepcopy(sample["branches"][1])
        branch.update(id=f"line{i}", name=f"Line {i}", **{"from": f"b{f}", "to": f"b{t}"})
        project["branches"].append(branch)
    project["name"] = f"{'meshed' if meshed else 'radial'}-{n}"
    return project


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before")
    parser.add_argument("after")
    parser.add_argument("--sizes", nargs="+", type=int, default=[25, 100, 300])
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()
    assert args.repeats > 0 and all(n > 1 for n in args.sizes)
    sample = json.loads(subprocess.check_output([args.before, "sample", "--compact"], text=True))
    cases = []
    for prefault in ["flat", "loadflow"]:
        project = copy.deepcopy(sample)
        project["prefault"] = prefault
        cases.append((f"sample-{prefault}", project, "all"))
    for n in args.sizes:
        for meshed in [False, True]:
            project = feeder(sample, n, meshed)
            cases.append((project["name"], project, "fault"))
    print("case,study,before_ms,after_ms,speedup,terminal_records")
    for name, project, study in cases:
        compare(invoke(args.before, project, study), invoke(args.after, project, study))
        times = [[], []]
        for repetition in range(args.repeats):
            # Alternate order to reduce systematic warm-up/thermal bias.
            for index in ([0, 1] if repetition % 2 == 0 else [1, 0]):
                result = invoke([args.before, args.after][index], project, study)
                times[index].append(result["elapsed_ms"])
        before, after = map(statistics.median, times)
        count = sum(len(bus["terminal_currents"]) for bus in result["fault"]["buses"])
        print(f"{name},{study},{before:.3f},{after:.3f},{before / after:.2f},{count}", flush=True)


if __name__ == "__main__":
    main()
