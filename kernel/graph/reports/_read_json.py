#!/usr/bin/env python3
import json

reports = [
    "branch_complexity_report.json",
    "branch_pressure_report.json",
    "callgraph_centrality_report.json",
    "dataflow_fanout_report.json",
    "merge_candidates_report.json",
    "path_redundancy_report.json",
    "reachability_report.json",
    "structural_hotspots_report.json",
    "dependency_cycle_report.json",
]

for rpt in reports:
    try:
        with open(rpt, 'r') as f:
            data = json.load(f)
        if isinstance(data, list):
            print(f"\n=== {rpt} === LIST len={len(data)}")
            if len(data) > 0:
                first = data[0]
                print(f"  first keys: {list(first.keys()) if isinstance(first, dict) else type(first)}")
        elif isinstance(data, dict):
            print(f"\n=== {rpt} === DICT keys={list(data.keys())[:10]}")
        else:
            print(f"\n=== {rpt} === type={type(data)}")
    except Exception as e:
        print(f"\n=== {rpt} ERROR: {e}")
