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
    r"^- \[([ x])\] \*\*(BK-WBS-((?:C)?\d{2})\.\d{2})\*\* (.+?)"
    r" — 상태=([A-Z])\([^)]*\); 선행=([^;]+); (.+)$"
)


def read_wbs():
    content = DOC.read_text(encoding="utf-8")
    rows = []
    seen = set()
    for number, line in enumerate(content.splitlines(), 1):
        if not line.startswith("- ["):
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
                     else [part.strip() for part in dependencies.split(",")]})
    if not rows:
        raise ValueError("WBS contains no leaf tasks")
    for row in rows:
        for dependency in row["dependencies"]:
            if dependency not in seen or dependency == row["id"]:
                raise ValueError(f"{row['id']}: invalid dependency {dependency}")
    by_id = {row["id"]: row for row in rows}
    visiting, visited = set(), set()

    def visit(identity):
        if identity in visiting:
            raise ValueError(f"dependency cycle at {identity}")
        if identity in visited:
            return
        visiting.add(identity)
        for dependency in by_id[identity]["dependencies"]:
            visit(dependency)
        visiting.remove(identity)
        visited.add(identity)

    for identity in by_id:
        visit(identity)
    known = set(re.findall(r"^\| (BK-\d{3}) \|", INVENTORY.read_text(encoding="utf-8"), re.M))
    mapped = set(re.findall(r"BK-\d{3}", content))
    if not known or known - mapped:
        raise ValueError(f"unmapped inventory features: {sorted(known - mapped)}")
    return rows, len(known)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    output = parser.add_mutually_exclusive_group()
    output.add_argument("--json", action="store_true", help="machine-readable counts")
    output.add_argument("--ready", action="store_true", help="pending items without a recorded external/decision/dependency blocker")
    args = parser.parse_args()
    rows, coverage = read_wbs()
    active = [row for row in rows if row["state"] != "C"]
    done = sum(row["state"] == "D" for row in active)
    states = collections.Counter(row["state"] for row in active)
    groups = {}
    for row in rows:
        group = groups.setdefault(row["group"], {"active": 0, "done": 0, "conditional": 0})
        if row["state"] == "C":
            group["conditional"] += 1
        else:
            group["active"] += 1
            group["done"] += row["state"] == "D"
    report = {"active": len(active), "done": done, "remaining": len(active) - done,
              "percent": round(done * 100 / len(active), 2) if active else None,
              "conditional": len(rows) - len(active), "states": dict(sorted(states.items())),
              "groups": groups, "mapped_features": coverage}
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    elif args.ready:
        done_ids = {row["id"] for row in rows if row["state"] == "D"}
        print("후보 목록: 문서의 실제 개발 의존성·파일 소유권·compiler 장벽도 적용해야 합니다.")
        for row in active:
            if row["state"] not in "DEPC" and set(row["dependencies"]) <= done_ids:
                print(f"{row['id']} [{row['state']}] {row['title']}")
    else:
        print(f"전체 WBS: {done}/{len(active)} 검증완료 ({report['percent']}%), 남음 {report['remaining']}, 조건부 {report['conditional']}")
        print("체크리스트 비율이며 공수·소요시간·최종 제품 완료율을 보증하지 않습니다.")
        for state, count in sorted(states.items()):
            print(f"  {state} {STATES[state]}: {count}")
        for name, group in groups.items():
            print(f"  {name}: {group['done']}/{group['active']} (조건부 {group['conditional']})")
        print(f"기능 ID coverage: {coverage}; ID/상태/선행 검사 통과")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as error:
        print(f"WBS 오류: {error}", file=sys.stderr)
        sys.exit(1)
