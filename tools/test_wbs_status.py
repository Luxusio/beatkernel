"""Independent literal-fixture tests for truthful hierarchical WBS reporting."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().with_name("wbs_status.py")
SPEC = importlib.util.spec_from_file_location("wbs_status", SOURCE)
wbs = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = wbs
SPEC.loader.exec_module(wbs)

INVENTORY = """# Feature inventory
| ID | Feature |
| --- | --- |
| BK-001 | Fixture feature |
"""

FLAT = """# Work breakdown
Coverage: BK-001
- [x] **BK-WBS-01.01** Accepted — 상태=D(검증완료); 선행=-; 근거=fixture accepted
- [ ] **BK-WBS-01.02** Implemented — 상태=V(구현됨·검증대기); 선행=BK-WBS-01.01; 다음=verification
- [ ] **BK-WBS-01.03** Integrating — 상태=W(구현·연동중); 선행=BK-WBS-01.02; 다음=integration
- [ ] **BK-WBS-02.01** Missing — 상태=N(미구현); 선행=-; 다음=implementation
- [ ] **BK-WBS-02.02** Equipment — 상태=E(환경·장비대기); 선행=-; 다음=equipment
- [ ] **BK-WBS-02.03** Policy — 상태=P(정책선택대기); 선행=-; 다음=decision
- [ ] **BK-WBS-02.04** Audit — 상태=U(현황감사필요); 선행=BK-WBS-01.01; 다음=audit
- [ ] **BK-WBS-C01.01** Conditional — 상태=C(조건부·미활성); 선행=-; 다음=activation
"""

FLAT_ROWS = [
    {"id": "BK-WBS-01.01", "group": "01", "title": "Accepted", "state": "D", "dependencies": [], "parent": None},
    {"id": "BK-WBS-01.02", "group": "01", "title": "Implemented", "state": "V", "dependencies": ["BK-WBS-01.01"], "parent": None},
    {"id": "BK-WBS-01.03", "group": "01", "title": "Integrating", "state": "W", "dependencies": ["BK-WBS-01.02"], "parent": None},
    {"id": "BK-WBS-02.01", "group": "02", "title": "Missing", "state": "N", "dependencies": [], "parent": None},
    {"id": "BK-WBS-02.02", "group": "02", "title": "Equipment", "state": "E", "dependencies": [], "parent": None},
    {"id": "BK-WBS-02.03", "group": "02", "title": "Policy", "state": "P", "dependencies": [], "parent": None},
    {"id": "BK-WBS-02.04", "group": "02", "title": "Audit", "state": "U", "dependencies": ["BK-WBS-01.01"], "parent": None},
    {"id": "BK-WBS-C01.01", "group": "C01", "title": "Conditional", "state": "C", "dependencies": [], "parent": None},
]

FLAT_REPORT = {
    "active": 7, "done": 1, "remaining": 6, "percent": 14.29,
    "conditional": 1, "states": {"D": 1, "E": 1, "N": 1, "P": 1, "U": 1, "V": 1, "W": 1},
    "groups": {"01": {"active": 3, "done": 1, "conditional": 0},
               "02": {"active": 4, "done": 0, "conditional": 0},
               "C01": {"active": 0, "done": 0, "conditional": 1}},
    "mapped_features": 1, "parents": 0, "implemented": 2,
    "requirements": {"active": 7, "done": 1, "remaining": 6, "percent": 14.29, "conditional": 1},
}

# Fifteen rows describe nine terminal work units and five original requirements.
# The six structural rows never enter terminal totals. Three terminal D plus one
# V yield four implemented work units; only two original requirements are D.
NESTED = """# Hierarchical breakdown
Coverage: BK-001
- [ ] **BK-WBS-01.01** Whole integration — 상태=W(구현·연동중); 선행=BK-WBS-02.01; 다음=whole acceptance
  - [x] **BK-WBS-01.01.01** Foundation — 상태=D(검증완료); 선행=-; 근거=foundation proof
  - [ ] **BK-WBS-01.01.02** Controller branch — 상태=W(구현·연동중); 선행=BK-WBS-03.01; 다음=controller acceptance
    - [ ] **BK-WBS-01.01.02.01** Controller assembly — 상태=W(구현·연동중); 선행=-; 다음=assembly acceptance
      - [ ] **BK-WBS-01.01.02.01.01** Controller code — 상태=V(구현됨·검증대기); 선행=BK-WBS-01.01.01; 다음=controller verification
      - [ ] **BK-WBS-01.01.02.01.02** Controller acceptance — 상태=N(미구현); 선행=-; 다음=acceptance fixture
    - [ ] **BK-WBS-01.01.02.02** Optional controller — 상태=C(조건부·미활성); 선행=-; 다음=activation
- [x] **BK-WBS-02.01** Prerequisite — 상태=D(검증완료); 선행=-; 근거=prerequisite proof
- [x] **BK-WBS-03.01** Accepted subsystem — 상태=D(검증완료); 선행=-; 근거=whole subsystem proof
  - [x] **BK-WBS-03.01.01** Accepted software — 상태=D(검증완료); 선행=-; 근거=software proof
  - [ ] **BK-WBS-03.01.02** Optional platform — 상태=C(조건부·미활성); 선행=-; 다음=activation
- [ ] **BK-WBS-C01.01** Conditional subsystem — 상태=C(조건부·미활성); 선행=-; 다음=activation
  - [ ] **BK-WBS-C01.01.01** Conditional implementation — 상태=C(조건부·미활성); 선행=-; 다음=activation
- [ ] **BK-WBS-04.01** Consumer requirement — 상태=W(구현·연동중); 선행=-; 다음=consumer acceptance
  - [ ] **BK-WBS-04.01.01** Consumer integration — 상태=W(구현·연동중); 선행=BK-WBS-03.01; 다음=consumer fixture
"""

NESTED_REPORT = {
    "active": 6, "done": 3, "remaining": 3, "percent": 50.0,
    "conditional": 3, "states": {"D": 3, "N": 1, "V": 1, "W": 1},
    "groups": {"01": {"active": 3, "done": 1, "conditional": 1},
               "02": {"active": 1, "done": 1, "conditional": 0},
               "03": {"active": 1, "done": 1, "conditional": 1},
               "C01": {"active": 0, "done": 0, "conditional": 1},
               "04": {"active": 1, "done": 0, "conditional": 0}},
    "mapped_features": 1, "parents": 6, "implemented": 4,
    "requirements": {"active": 4, "done": 2, "remaining": 2, "percent": 50.0, "conditional": 1},
}

PENDING_PARENT = """# Parent fixture
Coverage: BK-001
- [ ] **BK-WBS-01.01** Full requirement — 상태=W(구현·연동중); 선행=-; 다음=full acceptance
- [ ] **BK-WBS-01.01.01** Work unit — 상태=N(미구현); 선행=-; 다음=implementation
"""


class FixtureCase(unittest.TestCase):
    def parse(self, content, inventory=INVENTORY):
        return wbs.parse_wbs(content, inventory)

    def assert_report(self, actual, expected):
        self.assertEqual(set(actual), set(expected))
        for key, value in expected.items():
            with self.subTest(field=key):
                if key == "requirements":
                    self.assertTrue(set(value) <= set(actual[key]))
                    for field, count in value.items():
                        self.assertEqual(actual[key][field], count)
                else:
                    self.assertEqual(actual[key], value)

    def assert_invalid(self, content, inventory=INVENTORY):
        with self.assertRaises(ValueError) as caught:
            self.parse(content, inventory)
        self.assertTrue(str(caught.exception).strip(), "invalid input needs a diagnostic")


class ParserTests(FixtureCase):
    def test_flat_public_row_shape_and_original_dependency_order(self):
        rows, coverage = self.parse(FLAT)
        self.assertEqual(rows, FLAT_ROWS)
        self.assertEqual(coverage, 1)

    def test_nested_parent_ids_are_inferred_at_arbitrary_depth(self):
        rows, coverage = self.parse(NESTED)
        self.assertEqual(len(rows), 15)
        self.assertEqual(coverage, 1)
        self.assertEqual([(row["id"], row["parent"]) for row in rows], [
            ("BK-WBS-01.01", None),
            ("BK-WBS-01.01.01", "BK-WBS-01.01"),
            ("BK-WBS-01.01.02", "BK-WBS-01.01"),
            ("BK-WBS-01.01.02.01", "BK-WBS-01.01.02"),
            ("BK-WBS-01.01.02.01.01", "BK-WBS-01.01.02.01"),
            ("BK-WBS-01.01.02.01.02", "BK-WBS-01.01.02.01"),
            ("BK-WBS-01.01.02.02", "BK-WBS-01.01.02"),
            ("BK-WBS-02.01", None), ("BK-WBS-03.01", None),
            ("BK-WBS-03.01.01", "BK-WBS-03.01"),
            ("BK-WBS-03.01.02", "BK-WBS-03.01"),
            ("BK-WBS-C01.01", None),
            ("BK-WBS-C01.01.01", "BK-WBS-C01.01"),
            ("BK-WBS-04.01", None),
            ("BK-WBS-04.01.01", "BK-WBS-04.01"),
        ])
        self.assertEqual(rows[4]["dependencies"], ["BK-WBS-01.01.01"])

    def test_children_can_precede_parent_without_reordering_rows(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-01.01.01.01** Terminal — 상태=N(미구현); 선행=-; 다음=implementation
- [ ] **BK-WBS-01.01.01** Branch — 상태=W(구현·연동중); 선행=-; 다음=branch acceptance
- [ ] **BK-WBS-01.01** Requirement — 상태=W(구현·연동중); 선행=-; 다음=whole acceptance
"""
        rows, _ = self.parse(content)
        self.assertEqual([row["id"] for row in rows], [
            "BK-WBS-01.01.01.01", "BK-WBS-01.01.01", "BK-WBS-01.01"])
        self.assertEqual([row["parent"] for row in rows], [
            "BK-WBS-01.01.01", "BK-WBS-01.01", None])

    def test_indentation_is_optional_and_does_not_define_parentage(self):
        content = """Coverage: BK-001
\t  - [ ] **BK-WBS-01.01** Indented root — 상태=W(구현·연동중); 선행=-; 다음=whole acceptance
- [ ] **BK-WBS-01.01.01** Unindented child — 상태=N(미구현); 선행=-; 다음=implementation
    - [ ] **BK-WBS-02.01** Independent root — 상태=N(미구현); 선행=-; 다음=implementation
"""
        rows, _ = self.parse(content)
        self.assertEqual([row["parent"] for row in rows], [None, "BK-WBS-01.01", None])
        self.assertEqual([row["group"] for row in rows], ["01", "01", "02"])

    def test_missing_immediate_parent_is_rejected_even_if_root_exists(self):
        for content in (
            "Coverage: BK-001\n- [ ] **BK-WBS-01.01.01** Orphan — 상태=N(미구현); 선행=-; 다음=implementation\n",
            PENDING_PARENT.replace("BK-WBS-01.01.01**", "BK-WBS-01.01.02.01**"),
        ):
            with self.subTest(content=content):
                self.assert_invalid(content)

    def test_invalid_root_and_child_id_components_are_rejected(self):
        for identity in ("BK-WBS-1.01", "BK-WBS-01.1", "BK-WBS-C1.01",
                         "BK-WBS-01.01.1", "BK-WBS-01.01.xyz", "BK-WBS-01..01",
                         "BK-WBS-01.01."):
            with self.subTest(identity=identity):
                self.assert_invalid(FLAT.replace("BK-WBS-02.01", identity))

    def test_duplicate_root_and_child_ids_are_rejected(self):
        for content, duplicate in (
            (FLAT, "- [ ] **BK-WBS-02.01** Duplicate — 상태=N(미구현); 선행=-; 다음=implementation\n"),
            (PENDING_PARENT, "- [ ] **BK-WBS-01.01.01** Duplicate — 상태=N(미구현); 선행=-; 다음=implementation\n"),
        ):
            with self.subTest(duplicate=duplicate):
                self.assert_invalid(content + duplicate)

    def test_unknown_state_and_malformed_rows_are_rejected(self):
        for content in (
            FLAT.replace("상태=N(미구현)", "상태=Z(unknown)"),
            FLAT.replace("**BK-WBS-02.01**", "BK-WBS-02.01"),
            FLAT.replace("선행=-; 다음=implementation", "선행=- 다음=implementation"),
            FLAT.replace("- [ ] **BK-WBS-02.01**", "- [X] **BK-WBS-02.01**"),
            FLAT.replace("; 다음=implementation", "; "),
            FLAT + "  - [ ] an indented malformed checkbox row\n",
        ):
            with self.subTest(content=content):
                self.assert_invalid(content)

    def test_checkbox_must_agree_with_d_state_for_roots_and_children(self):
        for content in (
            FLAT.replace("- [x] **BK-WBS-01.01**", "- [ ] **BK-WBS-01.01**"),
            FLAT.replace("- [ ] **BK-WBS-02.01**", "- [x] **BK-WBS-02.01**"),
            NESTED.replace("- [x] **BK-WBS-01.01.01**", "- [ ] **BK-WBS-01.01.01**"),
            NESTED.replace("- [ ] **BK-WBS-01.01.02.01.01**", "- [x] **BK-WBS-01.01.02.01.01**"),
        ):
            with self.subTest(content=content):
                self.assert_invalid(content)

    def test_completed_roots_and_children_require_evidence_prefix(self):
        for content in (
            FLAT.replace("근거=fixture accepted", "다음=unverified claim"),
            NESTED.replace("근거=whole subsystem proof", "다음=unverified whole claim"),
            NESTED.replace("근거=software proof", "다음=unverified child claim"),
        ):
            with self.subTest(content=content):
                self.assert_invalid(content)

    def test_empty_wbs_cannot_become_a_zero_work_success(self):
        for content in ("", "# Only narrative\nCoverage: BK-001\n"):
            with self.subTest(content=content):
                self.assert_invalid(content)

    def test_empty_or_malformed_inventory_is_rejected(self):
        for inventory in ("", "# No feature rows\n", "| BK-01 | malformed ID |\n",
                          "| BK-0001 | malformed ID |\n"):
            with self.subTest(inventory=inventory):
                self.assert_invalid(FLAT, inventory)

    def test_all_inventory_features_must_be_mapped(self):
        inventory = INVENTORY + "| BK-002 | Unmapped feature |\n"
        with self.assertRaises(ValueError) as caught:
            self.parse(FLAT, inventory)
        self.assertIn("BK-002", str(caught.exception))

    def test_inventory_coverage_counts_unique_features(self):
        inventory = INVENTORY + "| BK-001 | Repeated historical mention |\n| BK-002 | Second feature |\n"
        rows, coverage = self.parse(FLAT + "Mapped also: BK-002\n", inventory)
        self.assertEqual(rows, FLAT_ROWS)
        self.assertEqual(coverage, 2)


class DependencyTests(FixtureCase):
    def test_multiple_prerequisites_preserve_declared_order_and_spaces(self):
        content = FLAT.replace("선행=BK-WBS-01.01; 다음=audit",
                               "선행=BK-WBS-01.01, BK-WBS-02.01; 다음=audit")
        rows, _ = self.parse(content)
        self.assertEqual(rows[6]["dependencies"], ["BK-WBS-01.01", "BK-WBS-02.01"])

    def test_unknown_and_direct_self_prerequisites_are_rejected(self):
        for dependency in ("BK-WBS-99.99", "BK-WBS-02.01"):
            content = FLAT.replace("선행=-; 다음=implementation",
                                   f"선행={dependency}; 다음=implementation")
            with self.subTest(dependency=dependency):
                self.assert_invalid(content)

    def test_direct_dependency_cycle_is_rejected(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-01.01** First — 상태=N(미구현); 선행=BK-WBS-02.01; 다음=implementation
- [ ] **BK-WBS-02.01** Second — 상태=N(미구현); 선행=BK-WBS-03.01; 다음=implementation
- [ ] **BK-WBS-03.01** Third — 상태=N(미구현); 선행=BK-WBS-01.01; 다음=implementation
"""
        self.assert_invalid(content)

    def test_child_dependency_on_own_parent_is_effective_cycle(self):
        self.assert_invalid(PENDING_PARENT.replace("선행=-; 다음=implementation",
                                                   "선행=BK-WBS-01.01; 다음=implementation"))

    def test_parent_dependency_on_own_descendant_is_effective_self_dependency(self):
        self.assert_invalid(PENDING_PARENT.replace("선행=-; 다음=full acceptance",
                                                   "선행=BK-WBS-01.01.01; 다음=full acceptance"))

    def test_inherited_ancestor_dependency_on_grandchild_is_rejected(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-01.01** Root — 상태=W(구현·연동중); 선행=BK-WBS-01.01.01.01; 다음=whole acceptance
- [ ] **BK-WBS-01.01.01** Branch — 상태=W(구현·연동중); 선행=-; 다음=branch acceptance
- [ ] **BK-WBS-01.01.01.01** Terminal — 상태=N(미구현); 선행=-; 다음=implementation
"""
        self.assert_invalid(content)

    def test_two_cross_parent_child_prerequisites_form_effective_cycle(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-01.01** First parent — 상태=W(구현·연동중); 선행=BK-WBS-02.01.01; 다음=whole acceptance
- [ ] **BK-WBS-01.01.01** First child — 상태=N(미구현); 선행=-; 다음=implementation
- [ ] **BK-WBS-02.01** Second parent — 상태=W(구현·연동중); 선행=BK-WBS-01.01.01; 다음=whole acceptance
- [ ] **BK-WBS-02.01.01** Second child — 상태=N(미구현); 선행=-; 다음=implementation
"""
        self.assert_invalid(content)

    def test_dependency_on_other_parent_whose_child_depends_back_is_cycle(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-01.01** Consumer — 상태=N(미구현); 선행=BK-WBS-02.01; 다음=implementation
- [ ] **BK-WBS-02.01** Provider — 상태=W(구현·연동중); 선행=-; 다음=whole acceptance
- [ ] **BK-WBS-02.01.01** Provider child — 상태=N(미구현); 선행=BK-WBS-01.01; 다음=implementation
"""
        self.assert_invalid(content)


class ReconciliationTests(FixtureCase):
    def test_d_parent_with_unfinished_active_terminal_scope_is_rejected(self):
        parent = PENDING_PARENT.replace("- [ ] **BK-WBS-01.01**", "- [x] **BK-WBS-01.01**")
        parent = parent.replace("상태=W(구현·연동중); 선행=-; 다음=full acceptance",
                                "상태=D(검증완료); 선행=-; 근거=claimed full proof")
        for state, label in (("N", "미구현"), ("V", "구현됨·검증대기"),
                             ("W", "구현·연동중"), ("E", "환경·장비대기"),
                             ("P", "정책선택대기"), ("U", "현황감사필요")):
            with self.subTest(state=state):
                self.assert_invalid(parent.replace("상태=N(미구현)", f"상태={state}({label})"))

    def test_non_d_parent_with_all_active_terminal_scopes_d_is_rejected(self):
        content = PENDING_PARENT.replace("- [ ] **BK-WBS-01.01.01**", "- [x] **BK-WBS-01.01.01**")
        content = content.replace("상태=N(미구현); 선행=-; 다음=implementation",
                                  "상태=D(검증완료); 선행=-; 근거=complete child proof")
        for state, label in (("W", "구현·연동중"), ("V", "구현됨·검증대기"),
                             ("N", "미구현"), ("E", "환경·장비대기"),
                             ("P", "정책선택대기"), ("U", "현황감사필요")):
            with self.subTest(state=state):
                self.assert_invalid(content.replace("상태=W(구현·연동중)", f"상태={state}({label})"))

    def test_active_parent_with_only_conditional_terminal_scopes_is_rejected(self):
        self.assert_invalid(PENDING_PARENT.replace("상태=N(미구현)", "상태=C(조건부·미활성)"))

    def test_active_descendants_under_conditional_parent_are_rejected(self):
        self.assert_invalid(PENDING_PARENT.replace("상태=W(구현·연동중)", "상태=C(조건부·미활성)"))
        nested = """Coverage: BK-001
- [ ] **BK-WBS-C01.01** Conditional root — 상태=C(조건부·미활성); 선행=-; 다음=activation
- [ ] **BK-WBS-C01.01.01** Conditional branch — 상태=C(조건부·미활성); 선행=-; 다음=activation
- [ ] **BK-WBS-C01.01.01.01** Active leaf — 상태=N(미구현); 선행=-; 다음=implementation
"""
        self.assert_invalid(nested)

    def test_unfinished_acceptance_child_preserves_valid_unfinished_parent(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-01.01** Full original criterion — 상태=W(구현·연동중); 선행=-; 다음=full product acceptance
- [x] **BK-WBS-01.01.01** Implemented foundation — 상태=D(검증완료); 선행=-; 근거=foundation proof only
- [ ] **BK-WBS-01.01.02** Remaining full acceptance — 상태=N(미구현); 선행=BK-WBS-01.01.01; 다음=original criterion verification
"""
        rows, coverage = self.parse(content)
        report = wbs.build_report(rows, coverage)
        self.assertEqual((report["active"], report["done"], report["remaining"]), (2, 1, 1))
        self.assertEqual(report["percent"], 50.0)
        self.assertEqual(report["requirements"]["done"], 0)
        self.assertEqual(report["requirements"]["remaining"], 1)
        self.assertEqual([row["id"] for row in wbs.ready_work(rows)], ["BK-WBS-01.01.02"])
        self.assertEqual(rows[0]["state"], "W", "reporting must not auto-promote the parent")

    def test_evidence_backed_d_parent_can_contain_inactive_optional_child(self):
        rows, _ = self.parse(NESTED)
        by_id = {row["id"]: row for row in rows}
        self.assertEqual(by_id["BK-WBS-03.01"]["state"], "D")
        self.assertEqual(by_id["BK-WBS-03.01.02"]["state"], "C")


class ReportTests(FixtureCase):
    def test_flat_existing_totals_groups_and_state_counts_are_preserved(self):
        rows, coverage = self.parse(FLAT)
        self.assert_report(wbs.build_report(rows, coverage), FLAT_REPORT)

    def test_deep_tree_counts_only_terminals_and_keeps_original_requirements(self):
        rows, coverage = self.parse(NESTED)
        self.assert_report(wbs.build_report(rows, coverage), NESTED_REPORT)

    def test_repartitioning_does_not_change_original_requirement_completion(self):
        unsplit = """Coverage: BK-001
- [ ] **BK-WBS-01.01** Whole integration — 상태=W(구현·연동중); 선행=BK-WBS-02.01; 다음=whole acceptance
- [x] **BK-WBS-02.01** Prerequisite — 상태=D(검증완료); 선행=-; 근거=prerequisite proof
- [x] **BK-WBS-03.01** Accepted subsystem — 상태=D(검증완료); 선행=-; 근거=whole subsystem proof
- [ ] **BK-WBS-C01.01** Conditional subsystem — 상태=C(조건부·미활성); 선행=-; 다음=activation
- [ ] **BK-WBS-04.01** Consumer requirement — 상태=W(구현·연동중); 선행=-; 다음=consumer acceptance
"""
        flat_rows, flat_coverage = self.parse(unsplit)
        split_rows, split_coverage = self.parse(NESTED)
        before = wbs.build_report(flat_rows, flat_coverage)
        after = wbs.build_report(split_rows, split_coverage)
        expected = {"active": 4, "done": 2, "remaining": 2, "percent": 50.0, "conditional": 1}
        for key, value in expected.items():
            self.assertEqual(before["requirements"][key], value)
            self.assertEqual(after["requirements"][key], value)
        self.assertEqual(before["active"], 4)
        self.assertEqual(after["active"], 6)

    def test_zero_active_conditional_tree_has_no_percent_or_implemented_work(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-C01.01** Conditional root — 상태=C(조건부·미활성); 선행=-; 다음=activation
- [ ] **BK-WBS-C01.01.01** Conditional leaf — 상태=C(조건부·미활성); 선행=-; 다음=activation
"""
        rows, coverage = self.parse(content)
        self.assert_report(wbs.build_report(rows, coverage), {
            "active": 0, "done": 0, "remaining": 0, "percent": None,
            "conditional": 1, "states": {},
            "groups": {"C01": {"active": 0, "done": 0, "conditional": 1}},
            "mapped_features": 1, "parents": 1, "implemented": 0,
            "requirements": {"active": 0, "done": 0, "remaining": 0, "percent": None, "conditional": 1},
        })
        self.assertEqual(wbs.ready_work(rows), [])
        self.assertEqual(wbs.in_progress_work(rows), [])

    def test_structural_d_and_v_are_excluded_from_implementation_count(self):
        content = """Coverage: BK-001
- [x] **BK-WBS-01.01** Accepted parent — 상태=D(검증완료); 선행=-; 근거=whole proof
- [x] **BK-WBS-01.01.01** Accepted leaf — 상태=D(검증완료); 선행=-; 근거=leaf proof
- [ ] **BK-WBS-02.01** Implemented parent — 상태=V(구현됨·검증대기); 선행=-; 다음=whole verification
- [ ] **BK-WBS-02.01.01** Implemented leaf — 상태=V(구현됨·검증대기); 선행=-; 다음=leaf verification
"""
        rows, coverage = self.parse(content)
        report = wbs.build_report(rows, coverage)
        self.assertEqual((report["active"], report["done"], report["implemented"], report["parents"]), (2, 1, 2, 2))
        self.assertEqual(report["states"], {"D": 1, "V": 1})
        self.assertEqual(report["requirements"]["active"], 2)
        self.assertEqual(report["requirements"]["done"], 1)


class WorkSelectionTests(FixtureCase):
    def test_flat_readiness_keeps_existing_u_candidate_and_blocks_non_d_prerequisite(self):
        rows, _ = self.parse(FLAT)
        ready = wbs.ready_work(rows)
        self.assertEqual([row["id"] for row in ready], ["BK-WBS-01.02", "BK-WBS-02.01", "BK-WBS-02.04"])
        self.assertEqual(ready, [FLAT_ROWS[1], FLAT_ROWS[3], FLAT_ROWS[6]])

    def test_readiness_uses_completed_whole_parent_and_all_ancestor_prerequisites(self):
        rows, _ = self.parse(NESTED)
        self.assertEqual([row["id"] for row in wbs.ready_work(rows)], [
            "BK-WBS-01.01.02.01.01", "BK-WBS-01.01.02.01.02", "BK-WBS-04.01.01"])
        blocked_root = NESTED.replace("- [x] **BK-WBS-02.01**", "- [ ] **BK-WBS-02.01**")
        blocked_root = blocked_root.replace("상태=D(검증완료); 선행=-; 근거=prerequisite proof",
                                            "상태=V(구현됨·검증대기); 선행=-; 다음=prerequisite verification")
        blocked_rows, _ = self.parse(blocked_root)
        self.assertEqual([row["id"] for row in wbs.ready_work(blocked_rows)], ["BK-WBS-02.01", "BK-WBS-04.01.01"])

    def test_readiness_requires_parent_declared_d_even_when_foundation_child_is_done(self):
        content = """Coverage: BK-001
- [ ] **BK-WBS-01.01** Provider — 상태=W(구현·연동중); 선행=-; 다음=whole acceptance
- [x] **BK-WBS-01.01.01** Foundation — 상태=D(검증완료); 선행=-; 근거=foundation proof
- [ ] **BK-WBS-01.01.02** Acceptance remainder — 상태=N(미구현); 선행=-; 다음=whole verification
- [ ] **BK-WBS-02.01** Needs whole provider — 상태=N(미구현); 선행=BK-WBS-01.01; 다음=implementation
- [ ] **BK-WBS-03.01** Needs only foundation — 상태=N(미구현); 선행=BK-WBS-01.01.01; 다음=implementation
"""
        rows, _ = self.parse(content)
        self.assertEqual([row["id"] for row in wbs.ready_work(rows)], ["BK-WBS-01.01.02", "BK-WBS-03.01"])

    def test_readiness_excludes_structural_rows_and_d_e_p_c_terminals(self):
        rows, _ = self.parse(NESTED)
        ready = wbs.ready_work(rows)
        self.assertEqual([row["state"] for row in ready], ["V", "N", "W"])
        self.assertTrue(all(row["parent"] is not None for row in ready))
        flat_rows, _ = self.parse(FLAT)
        self.assertEqual([row["state"] for row in wbs.ready_work(flat_rows)], ["V", "N", "U"])

    def test_in_progress_lists_terminal_w_v_n_regardless_of_blocked_prerequisite(self):
        rows, _ = self.parse(FLAT)
        selected = wbs.in_progress_work(rows)
        self.assertEqual(selected, [FLAT_ROWS[1], FLAT_ROWS[2], FLAT_ROWS[3]])
        self.assertEqual([row["state"] for row in selected], ["V", "W", "N"])
        nested_rows, _ = self.parse(NESTED)
        self.assertEqual([row["id"] for row in wbs.in_progress_work(nested_rows)], [
            "BK-WBS-01.01.02.01.01", "BK-WBS-01.01.02.01.02", "BK-WBS-04.01.01"])


class FileAndCliTests(FixtureCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.doc = Path(directory.name) / "work-breakdown.md"
        self.inventory = Path(directory.name) / "inventory.md"
        self.doc.write_text(NESTED, encoding="utf-8")
        self.inventory.write_text(INVENTORY, encoding="utf-8")
        self.addCleanup(mock.patch.stopall)
        mock.patch.object(wbs, "DOC", self.doc).start()
        mock.patch.object(wbs, "INVENTORY", self.inventory).start()

    def cli(self, *arguments):
        output = io.StringIO()
        error = io.StringIO()
        with mock.patch.object(sys, "argv", [str(SOURCE), *arguments]), contextlib.redirect_stdout(output), contextlib.redirect_stderr(error):
            wbs.main()
        self.assertEqual(error.getvalue(), "")
        return output.getvalue()

    def test_read_wbs_preserves_tuple_interface_and_reads_only_supplied_docs(self):
        rows, coverage = wbs.read_wbs()
        self.assertEqual((rows, coverage), self.parse(NESTED))
        self.assert_report(wbs.build_report(rows, coverage), NESTED_REPORT)
        self.assertEqual(self.doc.read_text(encoding="utf-8"), NESTED)
        self.assertEqual(self.inventory.read_text(encoding="utf-8"), INVENTORY)

    def test_read_wbs_propagates_file_read_and_parse_errors(self):
        self.doc.unlink()
        with self.assertRaises(OSError):
            wbs.read_wbs()
        self.doc.write_text("Coverage: BK-001\n- [ ] malformed row\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            wbs.read_wbs()
        self.doc.write_text(NESTED, encoding="utf-8")
        self.inventory.unlink()
        with self.assertRaises(OSError):
            wbs.read_wbs()

    def test_default_cli_shows_terminal_and_original_requirement_fractions(self):
        output = self.cli()
        self.assertEqual(output.splitlines()[:2], [
            "전체 WBS 말단 실행 단위: 3/6 검증완료 (50.0%), 남음 3, 조건부 3",
            "원래 제품 요구사항: 2/4 검증완료 (50.0%), 남음 2, 조건부 1",
        ])

    def test_json_cli_matches_literal_report(self):
        self.assert_report(json.loads(self.cli("--json")), NESTED_REPORT)

    def test_ready_cli_returns_only_ready_terminal_ids(self):
        output = self.cli("--ready")
        self.assertEqual(re.findall(r"BK-WBS-(?:C)?\d{2}(?:\.\d{2})+", output), [
            "BK-WBS-01.01.02.01.01", "BK-WBS-01.01.02.01.02", "BK-WBS-04.01.01"])

    def test_in_progress_cli_returns_only_terminal_w_v_n_ids(self):
        self.doc.write_text(FLAT, encoding="utf-8")
        output = self.cli("--in-progress")
        self.assertEqual(re.findall(r"BK-WBS-(?:C)?\d{2}(?:\.\d{2})+", output), [
            "BK-WBS-01.02", "BK-WBS-01.03", "BK-WBS-02.01"])

    def test_output_modes_are_mutually_exclusive_before_file_reads(self):
        for arguments in (("--json", "--ready"), ("--json", "--in-progress"),
                          ("--ready", "--in-progress")):
            error = io.StringIO()
            with self.subTest(arguments=arguments), mock.patch.object(wbs, "read_wbs") as reader, mock.patch.object(sys, "argv", [str(SOURCE), *arguments]), contextlib.redirect_stderr(error):
                with self.assertRaises(SystemExit) as caught:
                    wbs.main()
            self.assertEqual(caught.exception.code, 2)
            self.assertTrue(error.getvalue().strip())
            reader.assert_not_called()

    def test_help_lists_all_modes_without_reading_docs(self):
        output = io.StringIO()
        with mock.patch.object(wbs, "read_wbs") as reader, mock.patch.object(sys, "argv", [str(SOURCE), "--help"]), contextlib.redirect_stdout(output):
            with self.assertRaises(SystemExit) as caught:
                wbs.main()
        self.assertEqual(caught.exception.code, 0)
        for option in ("--json", "--ready", "--in-progress"):
            self.assertIn(option, output.getvalue())
        reader.assert_not_called()

    def test_script_boundary_turns_read_error_into_nonzero_actionable_exit(self):
        output = io.StringIO()
        error = io.StringIO()
        with mock.patch.object(Path, "read_text", side_effect=OSError("fixture docs unavailable")), mock.patch.object(sys, "argv", [str(SOURCE)]), contextlib.redirect_stdout(output), contextlib.redirect_stderr(error):
            with self.assertRaises(SystemExit) as caught:
                runpy.run_path(str(SOURCE), run_name="__main__")
        self.assertEqual(caught.exception.code, 1)
        self.assertEqual(output.getvalue(), "")
        self.assertIn("WBS", error.getvalue())
        self.assertIn("fixture docs unavailable", error.getvalue())

    def test_script_boundary_turns_parse_error_into_nonzero_without_partial_report(self):
        def fixture_read(path, *args, **kwargs):
            if path.name == "REQ__bms-player-work-breakdown.md":
                return "Coverage: BK-001\n- [ ] malformed row\n"
            return INVENTORY

        output = io.StringIO()
        error = io.StringIO()
        with mock.patch.object(Path, "read_text", autospec=True, side_effect=fixture_read), mock.patch.object(sys, "argv", [str(SOURCE), "--json"]), contextlib.redirect_stdout(output), contextlib.redirect_stderr(error):
            with self.assertRaises(SystemExit) as caught:
                runpy.run_path(str(SOURCE), run_name="__main__")
        self.assertEqual(caught.exception.code, 1)
        self.assertEqual(output.getvalue(), "")
        self.assertIn("WBS", error.getvalue())
        self.assertTrue(error.getvalue().strip())


class ProcessCliTests(FixtureCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repository = Path(directory.name)
        tools = self.repository / "tools"
        documents = self.repository / "doc" / "kernel"
        tools.mkdir()
        documents.mkdir(parents=True)
        self.script = tools / "wbs_status.py"
        self.script.write_bytes(SOURCE.read_bytes())
        self.doc = documents / "REQ__bms-player-work-breakdown.md"
        self.inventory = documents / "REQ__bms-player-progress.md"

    def process(self, *arguments):
        # An actual interpreter resolves ROOT from the copied production script.
        # Running from tools also prevents a cwd-based reader from finding doc/.
        return subprocess.run([sys.executable, str(self.script), *arguments],
                              cwd=self.script.parent, capture_output=True,
                              text=True, encoding="utf-8", timeout=5, check=False)

    def test_real_entrypoint_all_four_modes_use_literal_fixture_oracles(self):
        cases = (
            ((), NESTED, [
                "전체 WBS 말단 실행 단위: 3/6 검증완료 (50.0%), 남음 3, 조건부 3",
                "원래 제품 요구사항: 2/4 검증완료 (50.0%), 남음 2, 조건부 1",
                "구조 부모: 6; 구현됨(D+V) 말단: 4",
            ]),
            (("--json",), NESTED, NESTED_REPORT),
            (("--ready",), FLAT, [
                "BK-WBS-01.02 [V] Implemented",
                "BK-WBS-02.01 [N] Missing",
                "BK-WBS-02.04 [U] Audit",
            ]),
            (("--in-progress",), FLAT, [
                "BK-WBS-01.02 [V] Implemented",
                "BK-WBS-01.03 [W] Integrating",
                "BK-WBS-02.01 [N] Missing",
            ]),
        )
        for arguments, content, expected in cases:
            with self.subTest(arguments=arguments):
                self.doc.write_text(content, encoding="utf-8")
                self.inventory.write_text(INVENTORY, encoding="utf-8")
                result = self.process(*arguments)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr, "")
                if arguments == ("--json",):
                    self.assert_report(json.loads(result.stdout), expected)
                elif not arguments:
                    self.assertEqual(result.stdout.splitlines()[:3], expected)
                else:
                    rows = [line for line in result.stdout.splitlines()
                            if line.startswith("BK-WBS-")]
                    self.assertEqual(rows, expected)
                self.assertEqual(self.doc.read_text(encoding="utf-8"), content)
                self.assertEqual(self.inventory.read_text(encoding="utf-8"), INVENTORY)

    def test_real_entrypoint_missing_malformed_and_conflicting_modes_exit_cleanly(self):
        cases = (
            ("missing WBS", None, INVENTORY, (), 1,
             "REQ__bms-player-work-breakdown.md"),
            ("missing inventory", NESTED, None, ("--json",), 1,
             "REQ__bms-player-progress.md"),
            ("malformed WBS", "Coverage: BK-001\n- [ ] malformed row\n",
             INVENTORY, ("--ready",), 1, "WBS 오류:"),
            ("malformed inventory", NESTED, "# No feature IDs\n",
             ("--in-progress",), 1, "WBS 오류:"),
            ("mode conflict before missing files", None, None,
             ("--json", "--ready"), 2, "--ready"),
        )
        for name, content, inventory, arguments, code, diagnostic in cases:
            with self.subTest(case=name):
                for path, text in ((self.doc, content), (self.inventory, inventory)):
                    if text is None:
                        if path.exists():
                            path.unlink()
                    else:
                        path.write_text(text, encoding="utf-8")
                result = self.process(*arguments)
                self.assertEqual(result.returncode, code)
                self.assertEqual(result.stdout, "")
                self.assertIn(diagnostic, result.stderr)


if __name__ == "__main__":
    unittest.main()
