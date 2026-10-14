#!/usr/bin/env python3
"""Count the canonical product WBS; never change files or grant acceptance."""
import argparse
import collections
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
DOC = ROOT / "doc/kernel/REQ__bms-player-work-breakdown.md"
INVENTORY = ROOT / "doc/kernel/REQ__bms-player-progress.md"
STATES = {
    "D": "검증완료", "V": "구현됨·검증대기", "W": "구현·연동중",
    "N": "미구현", "E": "환경·장비대기", "P": "정책선택대기",
    "U": "현황감사필요", "C": "조건부·미활성",
}
LEAF = re.compile(
    r"^[ \t]*- \[([ x])\] \*\*(BK-WBS-((?:C)?\d{2})\.\d{2}(?:\.\d{2})*)\*\* (.+?)"
    r" — 상태=([A-Z])\([^)]*\); 선행=([^;]+); (.+)$"
)


def _work_tree(rows):
    by_id = {row["id"]: row for row in rows}
    children = {identity: [] for identity in by_id}
    for row in rows:
        parent = row["parent"]
        if parent is not None:
            if parent not in by_id:
                raise ValueError(f"{row['id']}: missing immediate parent {parent}")
            children[parent].append(row["id"])
    return by_id, children


def _active_leaves(identity, by_id, children):
    if not children[identity]:
        return [] if by_id[identity]["state"] == "C" else [identity]
    leaves = []
    for child in children[identity]:
        leaves.extend(_active_leaves(child, by_id, children))
    return leaves


def _check_cycles(graph):
    visiting, visited = set(), set()

    def visit(identity):
        if identity in visiting:
            raise ValueError(f"dependency cycle at {identity}")
        if identity in visited:
            return
        visiting.add(identity)
        for dependency in graph[identity]:
            visit(dependency)
        visiting.remove(identity)
        visited.add(identity)

    for identity in graph:
        visit(identity)


def parse_wbs(content, inventory_text):
    rows = []
    seen = set()
    for number, line in enumerate(content.splitlines(), 1):
        if not line.lstrip(" \t").startswith("- ["):
            continue
        match = LEAF.fullmatch(line)
        if not match:
            raise ValueError(f"{DOC.name}:{number}: malformed WBS leaf")
        checked, identity, group, title, state, dependencies, evidence = match.groups()
        if identity in seen or state not in STATES:
            raise ValueError(f"{identity}: duplicate ID or unknown state")
        if (checked == "x") != (state == "D"):
            raise ValueError(f"{identity}: checkbox/state disagreement")
        if state == "D" and not evidence.startswith("근거="):
            raise ValueError(f"{identity}: completed leaf lacks evidence")
        seen.add(identity)
        rows.append({"id": identity, "group": group, "title": title,
                     "state": state, "dependencies": [] if dependencies == "-"
                     else [part.strip() for part in dependencies.split(",")],
                     "parent": identity.rsplit(".", 1)[0] if identity.count(".") > 1
                     else None})
    if not rows:
        raise ValueError("WBS contains no leaf tasks")
    by_id, children = _work_tree(rows)
    for row in rows:
        for dependency in row["dependencies"]:
            if dependency not in seen or dependency == row["id"]:
                raise ValueError(f"{row['id']}: invalid dependency {dependency}")
    declared = {row["id"]: row["dependencies"] for row in rows}
    _check_cycles(declared)
    effective = {}
    for row in rows:
        dependencies = set(row["dependencies"]) | set(children[row["id"]])
        parent = row["parent"]
        while parent is not None:
            ancestor = by_id[parent]
            if row["state"] != "C" and ancestor["state"] == "C":
                raise ValueError(f"{row['id']}: active child under conditional parent {parent}")
            dependencies.update(ancestor["dependencies"])
            parent = ancestor["parent"]
        if row["id"] in dependencies:
            raise ValueError(f"{row['id']}: effective self-dependency")
        effective[row["id"]] = dependencies
    _check_cycles(effective)
    for row in rows:
        if not children[row["id"]] or row["state"] == "C":
            continue
        leaves = _active_leaves(row["id"], by_id, children)
        if not leaves:
            raise ValueError(f"{row['id']}: active parent has no active leaf tasks")
        all_done = all(by_id[identity]["state"] == "D" for identity in leaves)
        if row["state"] == "D" and not all_done:
            raise ValueError(f"{row['id']}: completed parent has unfinished active leaves")
        if row["state"] != "D" and all_done:
            raise ValueError(f"{row['id']}: unfinished parent has all active leaves completed")
    known = set(re.findall(r"^\| (BK-\d{3}) \|", inventory_text, re.M))
    mapped = set(re.findall(r"BK-\d{3}", content))
    if not known or known - mapped:
        raise ValueError(f"unmapped inventory features: {sorted(known - mapped)}")
    return rows, len(known)


def read_wbs():
    return parse_wbs(DOC.read_text(encoding="utf-8"),
                     INVENTORY.read_text(encoding="utf-8"))


def _counts(rows):
    active = [row for row in rows if row["state"] != "C"]
    done = sum(row["state"] == "D" for row in active)
    return {"active": len(active), "done": done, "remaining": len(active) - done,
            "percent": round(done * 100 / len(active), 2) if active else None,
            "conditional": len(rows) - len(active)}


def build_report(rows, coverage):
    _, children = _work_tree(rows)
    leaves = [row for row in rows if not children[row["id"]]]
    active = [row for row in leaves if row["state"] != "C"]
    states = collections.Counter(row["state"] for row in active)
    groups = {}
    for row in leaves:
        group = groups.setdefault(row["group"], {"active": 0, "done": 0, "conditional": 0})
        if row["state"] == "C":
            group["conditional"] += 1
        else:
            group["active"] += 1
            group["done"] += row["state"] == "D"
    report = _counts(leaves)
    report.update({"states": dict(sorted(states.items())), "groups": groups,
                   "mapped_features": coverage, "parents": len(rows) - len(leaves),
                   "implemented": sum(row["state"] in "DV" for row in active),
                   "requirements": _counts([row for row in rows if row["parent"] is None])})
    return report


def ready_work(rows):
    by_id, children = _work_tree(rows)

    def complete(identity):
        return by_id[identity]["state"] == "D" and all(
            by_id[leaf]["state"] == "D"
            for leaf in _active_leaves(identity, by_id, children))

    ready = []
    for row in rows:
        if children[row["id"]] or row["state"] in "DEPC":
            continue
        dependencies = set(row["dependencies"])
        parent = row["parent"]
        while parent is not None:
            ancestor = by_id[parent]
            dependencies.update(ancestor["dependencies"])
            parent = ancestor["parent"]
        if all(complete(identity) for identity in dependencies):
            ready.append(row)
    return ready


def in_progress_work(rows):
    _, children = _work_tree(rows)
    return [row for row in rows if not children[row["id"]] and row["state"] in "WVN"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    output = parser.add_mutually_exclusive_group()
    output.add_argument("--json", action="store_true", help="machine-readable counts")
    output.add_argument("--ready", action="store_true", help="pending items without a recorded external/decision/dependency blocker")
    output.add_argument("--in-progress", action="store_true", help="terminal work units in W/V/N states")
    args = parser.parse_args()
    rows, coverage = read_wbs()
    report = build_report(rows, coverage)
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    elif args.ready:
        print("후보 목록: 문서의 실제 개발 의존성·파일 소유권·compiler 장벽도 적용해야 합니다.")
        for row in ready_work(rows):
            print(f"{row['id']} [{row['state']}] {row['title']}")
    elif args.in_progress:
        print("진행 중 실행 단위: W 구현·연동중, V 구현됨·검증대기, N 미구현")
        for row in in_progress_work(rows):
            print(f"{row['id']} [{row['state']}] {row['title']}")
    else:
        print(f"전체 WBS 말단 실행 단위: {report['done']}/{report['active']} 검증완료 ({report['percent']}%), 남음 {report['remaining']}, 조건부 {report['conditional']}")
        requirements = report["requirements"]
        print(f"원래 제품 요구사항: {requirements['done']}/{requirements['active']} 검증완료 ({requirements['percent']}%), 남음 {requirements['remaining']}, 조건부 {requirements['conditional']}")
        print(f"구조 부모: {report['parents']}; 구현됨(D+V) 말단: {report['implemented']}")
        print("체크리스트 비율이며 공수·소요시간·최종 제품 완료율을 보증하지 않습니다.")
        for state, count in report["states"].items():
            print(f"  {state} {STATES[state]}: {count}")
        for name, group in report["groups"].items():
            print(f"  {name}: {group['done']}/{group['active']} (조건부 {group['conditional']})")
        print(f"기능 ID coverage: {coverage}; ID/상태/선행 검사 통과")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"WBS 오류: {error}", file=sys.stderr)
        sys.exit(1)
