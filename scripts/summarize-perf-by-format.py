#!/usr/bin/env python3
"""Print format breakdowns from completed performance trial directories."""
import argparse
import json
from pathlib import Path
from _perf_sampling import per_format_groups_from_events, load_events_ndjson

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('runs', nargs='+', type=Path)
args = parser.parse_args()
for directory in args.runs:
    print(json.dumps(dict(run=str(directory), per_format=per_format_groups_from_events(
        load_events_ndjson(directory / 'events.ndjson'))), indent=2))
