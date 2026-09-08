#!/usr/bin/env python3
import json
import sys
from pathlib import Path


def load_ndjson(path: Path) -> list[dict]:
    if not path.exists():
        return []
    events = []
    for line_number, line in enumerate(path.read_text().splitlines(), start=1):
        if not line.strip():
            continue
        try:
            events.append(json.loads(line))
        except json.JSONDecodeError as error:
            raise SystemExit(f"malformed events.ndjson line {line_number}: {error}") from error
    return events


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: summarize-artifact.py <artifact-dir>", file=sys.stderr)
        return 2

    artifact_dir = Path(sys.argv[1]).expanduser().resolve()
    report_path = artifact_dir / "report.json"
    events_path = artifact_dir / "events.ndjson"

    if not artifact_dir.is_dir():
        print(f"artifact directory not found: {artifact_dir}", file=sys.stderr)
        return 1
    if not report_path.exists():
        print(f"report.json not found: {report_path}", file=sys.stderr)
        return 1

    report = json.loads(report_path.read_text())
    events = load_ndjson(events_path)
    assertions = report.get("assertions", [])
    failed_assertions = [item for item in assertions if not item.get("passed")]
    actions = [event for event in events if event.get("type") == "action"]
    targets = [event for event in events if event.get("type") == "observation.target"]
    facts = [event for event in events if event.get("type") == "observation.fact"]
    frames = report.get("frames", [])

    print(f"scenario: {report.get('scenario', '<unknown>')}")
    print(f"status: {report.get('status', '<unknown>')}")
    print(f"artifact: {artifact_dir}")
    first_failure = report.get("first_failure")
    if first_failure:
        print(f"first_failure: {first_failure}")

    if assertions:
        print("assertions:")
        for assertion in assertions:
            marker = "pass" if assertion.get("passed") else "fail"
            print(f"- {marker}: {assertion.get('name', '<unnamed>')}")

    if actions:
        print("actions:")
        for action in actions:
            payload = action.get("payload", {})
            screen = payload.get("screen", {})
            suffix = ""
            if screen:
                suffix = f" @ screen({screen.get('x')}, {screen.get('y')})"
            print(f"- {payload.get('name', '<unnamed>')}{suffix}")

    if targets:
        print("targets:")
        for target in targets:
            payload = target.get("payload", {})
            rect = payload.get("rect", {})
            center = rect.get("center", {})
            print(
                f"- {payload.get('name', '<unnamed>')} "
                f"{payload.get('kind', '<unknown>')} "
                f"center=({center.get('x')}, {center.get('y')})"
            )

    if facts:
        print("facts:")
        for fact in facts:
            print(f"- {fact.get('payload', {}).get('name', '<unnamed>')}")

    if frames:
        print("frames:")
        for frame in frames:
            print(f"- {frame}")

    return 1 if failed_assertions or report.get("status") == "fail" else 0


if __name__ == "__main__":
    raise SystemExit(main())
