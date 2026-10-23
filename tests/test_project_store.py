"""Local project provenance and append-only review invariants."""

import hashlib
import json
from copy import deepcopy
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import sqlite3
import tempfile
from threading import Barrier
import unittest

from genemapper_open.project_store import (
    EXPORT_FORMAT, MAX_JSON_BYTES, ProjectStore, ProjectStoreError,
)


class ProjectStoreTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.path = self.root / "project.sqlite"
        self.store = ProjectStore.create(self.path, "Research project")
        self.source_sha = hashlib.sha256(b"source trace bytes kept outside the store").hexdigest()
        self.sample = self.store.add_sample(self.source_sha, {"file_name": "example.fsa", "kit": "explicit"})
        self.machine = {
            "calls": [{"id": "call-1", "label": "12", "marker": "example"},
                      {"id": "call-2", "label": "13"}],
            "peaks": [{"id": "peak-1", "scan": 100, "height": 1200}],
            "quality": {"accepted": False, "reason": "research only"},
        }
        self.analysis = self.add_analysis()

    def tearDown(self):
        self.store.close()
        self.temporary.cleanup()

    def add_analysis(self, **kwargs):
        values = dict(sample_id=self.sample, input_sha256=self.source_sha,
                      algorithm={"name": "independent-test", "version": "0.5"},
                      settings={"threshold": 100, "kit_sha256": "b" * 64},
                      result=self.machine, status="quality-rejected")
        values.update(kwargs)
        return self.store.add_analysis(**values)

    def review(self, action, **kwargs):
        values = dict(actor="researcher", reason="Checked trace manually",
                      target_kind="call", target_id="call-1")
        values.update(kwargs)
        return self.store.append_review(self.analysis, action, **values)

    def overlay(self):
        return self.store.effective_analysis(self.analysis)["review_targets"][0]

    def test_create_open_and_provenance(self):
        analysis = self.store.get_analysis(self.analysis)
        self.assertEqual(analysis["input_sha256"], self.source_sha)
        for field in ("algorithm", "settings", "result"):
            encoded = json.dumps(analysis[field], sort_keys=True, separators=(",", ":"), ensure_ascii=False)
            self.assertEqual(analysis[field + "_sha256"], hashlib.sha256(encoded.encode()).hexdigest())
        with ProjectStore.open(self.path) as other:
            self.assertEqual(other.export_json(), self.store.export_json())
            self.assertEqual(other.project["name"], "Research project")

    def test_creation_never_overwrites_existing_destination(self):
        before = self.path.read_bytes()
        with self.assertRaises(FileExistsError):
            ProjectStore.create(self.path, "replacement")
        self.assertEqual(self.path.read_bytes(), before)
        empty = self.root / "empty.sqlite"
        empty.touch()
        with self.assertRaises(FileExistsError):
            ProjectStore.create(empty, "replacement")
        self.assertEqual(empty.read_bytes(), b"")

    def test_missing_or_original_format_not_discovered_or_created(self):
        missing = self.root / "missing.sqlite"
        with self.assertRaises(ProjectStoreError):
            ProjectStore.open(missing)
        self.assertFalse(missing.exists())
        original = self.root / "vendor.ser"
        original.write_bytes(b"\xac\xed\x00\x05not a project")
        with self.assertRaises(ProjectStoreError):
            ProjectStore.open(original)
        self.assertTrue(original.read_bytes().startswith(b"\xac\xed"))

    def test_input_hash_binding_and_transaction_survives_failure(self):
        before = self.store.export_json()
        for changes in ({"input_sha256": "0" * 64}, {"sample_id": "unknown"},
                        {"input_sha256": "ABC"}, {"algorithm": {"name": "missing-version"}}):
            with self.assertRaises(ProjectStoreError):
                self.add_analysis(**changes)
            self.assertEqual(self.store.export_json(), before)
        second = self.add_analysis(status="completed")
        self.assertEqual(len(self.store.list_analyses()), 2)
        self.assertEqual(self.store.get_analysis(second)["status"], "completed")

    def test_result_and_return_values_are_detached_snapshots(self):
        self.machine["calls"][0]["label"] = "changed outside store"
        result = self.store.get_analysis(self.analysis)
        result["result"]["calls"][0]["label"] = "changed returned data"
        self.assertEqual(self.store.get_analysis(self.analysis)["result"]["calls"][0]["label"], "12")

    def test_database_triggers_reject_record_changes(self):
        for table in ("store_info", "sample_records", "analysis_records", "review_log"):
            if table == "review_log":
                self.review("accept")
            for statement in (f"DELETE FROM {table}", f"UPDATE {table} SET rowid=rowid"):
                with self.assertRaisesRegex(sqlite3.IntegrityError, "immutable"):
                    with self.store._connection:
                        self.store._connection.execute(statement)
        self.assertEqual(len(self.store.list_samples()), 1)

    def test_review_folds_separately_from_machine_result_and_machine_status(self):
        original = self.store.get_analysis(self.analysis)
        self.review("accept")
        self.review("change-label", value="12.2")
        self.review("note", value="Shoulder inspected")
        state = self.store.effective_analysis(self.analysis)
        self.assertEqual(state["machine_status"], "quality-rejected")
        self.assertEqual(state["machine_result"]["calls"][0]["label"], "12")
        self.assertEqual(self.overlay()["label"], "12.2")
        self.assertTrue(self.overlay()["accepted"])
        self.assertEqual(self.overlay()["notes"][0]["text"], "Shoulder inspected")
        self.assertEqual(self.store.get_analysis(self.analysis), original)

    def test_review_order_is_append_sequence_even_with_backdated_timestamps(self):
        self.review("accept", created_at="2026-10-22T13:00:00+03:00")
        self.review("exclude", created_at="2026-10-20T00:00:00Z")
        events = self.store.review_events(self.analysis)
        self.assertEqual([event["sequence"] for event in events], [1, 2])
        self.assertFalse(self.overlay()["accepted"])

    def quality_analysis(self, *, status="evaluated_subset", marker_status="evaluated_subset"):
        result = deepcopy(self.machine)
        result["calls"][1]["marker"] = "other"
        result["quality"]["marker_gq"] = {
            "status": status, "sample_category": 0,
            "markers": [{"marker": "example", "status": marker_status,
                         "score": {"gq": 1.0}, "category": {"code": 0},
                         "call_scores": [{"call_id": "call-1", "after_sample_sq": 0.9}]},
                        {"marker": "other", "status": "evaluated_subset", "score": {"gq": 1.0}}],
        }
        self.analysis = self.add_analysis(result=result)
        return self.store.get_analysis(self.analysis)

    def test_review_quality_relabel_and_exclude_invalidate_without_rewriting_machine(self):
        original = self.quality_analysis()
        relabel = self.review("change-label", value="14")
        exclude = self.review("exclude", target_id="call-2")
        state = self.store.effective_analysis(self.analysis)
        quality = state["review_quality"]
        self.assertEqual(quality, {
            "policy": "net_call_interpretation",
            "affected_markers": ["example", "other"],
            "active_event_ids": [relabel, exclude],
            "markers": [{"marker": "example", "freshness": "stale",
                         "machine_status": "evaluated_subset", "active_event_ids": [relabel]},
                        {"marker": "other", "freshness": "stale",
                         "machine_status": "evaluated_subset", "active_event_ids": [exclude]}],
            "sample_summary": {"freshness": "stale", "machine_status": "evaluated_subset"},
        })
        self.assertEqual(state["machine_result"], original["result"])
        self.assertEqual(self.store.get_analysis(self.analysis), original)

    def test_review_quality_net_effect_ignores_noops_and_superseded_events(self):
        self.quality_analysis()
        baseline = self.store.effective_analysis(self.analysis)["review_quality"]
        self.review("note", value="Inspected")
        self.review("accept")
        self.review("exclude", target_kind="peak", target_id="peak-1")
        self.review("change-label", value="12")
        self.assertEqual(self.store.effective_analysis(self.analysis)["review_quality"], baseline)
        self.review("exclude")
        self.review("accept")
        self.review("change-label", value="14")
        self.review("change-label", value="12")
        self.assertEqual(self.store.effective_analysis(self.analysis)["review_quality"], baseline)
        self.review("change-label", value="15")
        latest = self.review("change-label", value="16")
        quality = self.store.effective_analysis(self.analysis)["review_quality"]
        self.assertEqual(quality["affected_markers"], ["example"])
        self.assertEqual(quality["active_event_ids"], [latest])
        self.assertEqual(quality["markers"][1]["freshness"], "unchanged")

    def test_review_quality_excluded_call_uses_exclusion_until_undo_reveals_label(self):
        self.quality_analysis()
        relabel = self.review("change-label", value="14")
        exclude = self.review("exclude")
        self.assertEqual(self.store.effective_analysis(self.analysis)["review_quality"]["active_event_ids"], [exclude])
        self.store.undo_review(self.analysis, exclude, actor="r", reason="Restore call")
        quality = self.store.effective_analysis(self.analysis)["review_quality"]
        self.assertEqual(quality["active_event_ids"], [relabel])
        with ProjectStore.from_json(self.store.export_json()) as imported:
            self.assertEqual(imported.effective_analysis(self.analysis)["review_quality"], quality)
        self.store.undo_review(self.analysis, relabel, actor="r", reason="Restore label")
        quality = self.store.effective_analysis(self.analysis)["review_quality"]
        self.assertEqual(quality["affected_markers"], [])
        self.assertEqual(quality["active_event_ids"], [])
        self.assertEqual(quality["sample_summary"]["freshness"], "unchanged")

    def test_review_quality_undo_preserves_original_global_or_marker_staleness(self):
        for global_status, row_status in (("stale", "evaluated_subset"), ("partial", "stale")):
            with self.subTest(global_status=global_status, row_status=row_status):
                self.quality_analysis(status=global_status, marker_status=row_status)
                baseline = self.store.effective_analysis(self.analysis)["review_quality"]
                event = self.review("change-label", value="14")
                self.store.undo_review(self.analysis, event, actor="r", reason="Restore")
                quality = self.store.effective_analysis(self.analysis)["review_quality"]
                self.assertEqual(quality, baseline)
                self.assertEqual(quality["markers"][0]["freshness"], "stale")
                self.assertEqual(quality["sample_summary"]["freshness"], "stale")
                self.assertEqual(quality["markers"][1]["freshness"],
                                 "stale" if global_status == "stale" else "unchanged")

    def test_review_quality_unassigned_call_and_missing_metrics_do_not_invent_quality(self):
        baseline = self.store.effective_analysis(self.analysis)["review_quality"]
        self.assertEqual(baseline["sample_summary"], {"freshness": "unchanged", "machine_status": None})
        event = self.review("exclude", target_id="call-2")
        quality = self.store.effective_analysis(self.analysis)["review_quality"]
        self.assertEqual(quality["affected_markers"], [])
        self.assertEqual(quality["active_event_ids"], [event])
        self.assertEqual(quality["sample_summary"], {"freshness": "stale", "machine_status": None})
        self.assertEqual(quality["markers"][0]["freshness"], "unchanged")

    def test_undo_is_new_event_with_reason_actor_and_restores_previous_effect(self):
        accepted = self.review("accept")
        excluded = self.review("exclude")
        relabel = self.review("change-label", value="14")
        before = self.store.review_events(self.analysis)
        reversal = self.store.undo_review(self.analysis, excluded, actor="reviewer-2", reason="Wrong target")
        events = self.store.review_events(self.analysis)
        self.assertEqual(events[:3], before)
        self.assertEqual(events[-1]["event_id"], reversal)
        self.assertEqual(events[-1]["reverses_event_id"], excluded)
        self.assertEqual(events[-1]["actor"], "reviewer-2")
        self.assertEqual(events[-1]["reason"], "Wrong target")
        self.assertTrue(self.overlay()["accepted"])
        self.assertEqual(self.overlay()["label"], "14")
        self.store.undo_review(self.analysis, accepted, actor="reviewer", reason="Clear acceptance")
        self.store.undo_review(self.analysis, relabel, actor="reviewer", reason="Restore machine label")
        self.assertIsNone(self.overlay()["accepted"])
        self.assertEqual(self.overlay()["label"], "12")

    def test_undo_rejects_double_cross_analysis_unknown_or_undo_target(self):
        event = self.review("accept")
        other = self.add_analysis()
        with self.assertRaises(ProjectStoreError):
            self.store.undo_review(other, event, actor="r", reason="Wrong analysis")
        undo = self.store.undo_review(self.analysis, event, actor="r", reason="Undo")
        for identifier in (event, undo, "unknown"):
            with self.assertRaises(ProjectStoreError):
                self.store.undo_review(self.analysis, identifier, actor="r", reason="Cannot undo")
        self.assertEqual(len(self.store.review_events()), 2)

    def test_analysis_notes_and_peak_review(self):
        event = self.store.append_review(self.analysis, "note", actor="r", reason="Context", value="Re-run requested")
        self.review("exclude", target_kind="peak", target_id="peak-1")
        state = self.store.effective_analysis(self.analysis)
        self.assertEqual(state["review_notes"], [{"event_id": event, "text": "Re-run requested"}])
        self.assertFalse(state["review_targets"][-1]["accepted"])

    def test_invalid_events_do_not_leave_partial_writes(self):
        cases = [dict(action="accept", reason=""), dict(action="note", value=None),
                 dict(action="change-label", value=""), dict(action="accept", value="ignored"),
                 dict(action="accept", target_id="missing"), dict(action="accept", actor=""),
                 dict(action="accept", created_at="2026-10-21T01:00:00"),
                 dict(action="accept", target_kind="analysis", target_id=None),
                 dict(action="exclude", reverses_event_id="missing"), dict(action="sql")]
        for case in cases:
            action = case.pop("action")
            with self.assertRaises(ProjectStoreError):
                self.review(action, **case)
            self.assertEqual(self.store.review_events(), [])
        self.review("accept")
        self.assertEqual(self.store.review_events()[0]["sequence"], 1)

    def test_duplicate_identifier_failure_does_not_overwrite(self):
        snapshot = self.store.export_json()
        with self.assertRaises(sqlite3.IntegrityError):
            self.add_analysis(analysis_id=self.analysis, status="replacement")
        with self.assertRaises(sqlite3.IntegrityError):
            self.store.add_sample(self.source_sha, {"file_name": "replacement"}, sample_id=self.sample)
        self.assertEqual(self.store.export_json(), snapshot)

    def test_parameterized_values_treat_sql_text_as_data(self):
        malicious = "'); DROP TABLE analysis_records; --"
        self.review("note", actor=malicious, value=malicious)
        self.assertEqual(self.store.review_events()[0]["actor"], malicious)
        self.assertEqual(self.overlay()["notes"][0]["text"], malicious)
        self.assertEqual(len(self.store.list_analyses()), 1)

    def test_json_and_target_validation(self):
        invalid_results = [{"calls": [{"id": "x"}, {"id": "x"}]}, {"calls": {}},
                           {"peaks": [{}]}, {"metric": float("nan")}, {1: "non-string key"},
                           {"raw": b"binary"}, {"large": "a" * MAX_JSON_BYTES}]
        nested = {}
        for _ in range(34):
            nested = {"nested": nested}
        invalid_results.append(nested)
        for result in invalid_results:
            with self.assertRaises(ProjectStoreError):
                self.add_analysis(result=result)
        with self.assertRaises(ProjectStoreError):
            self.store.add_sample(self.source_sha, {"raw_trace": [1, 2, 3]})
        self.assertEqual(len(self.store.list_analyses()), 1)

    def test_round_trip_preserves_audit_and_effective_view(self):
        event = self.review("exclude")
        self.review("change-label", value="15")
        self.store.undo_review(self.analysis, event, actor="reviewer", reason="Restored")
        document = self.store.export_json()
        self.assertEqual(json.loads(document)["format"], EXPORT_FORMAT)
        target = self.root / "import.sqlite"
        with ProjectStore.import_json(target, document) as imported:
            self.assertEqual(imported.export_json(), document)
            self.assertEqual(imported.effective_analysis(self.analysis), self.store.effective_analysis(self.analysis))
        with ProjectStore.open(target) as reopened:
            self.assertEqual(reopened.export_json(), document)

    def test_import_refuses_existing_destination_without_overwriting(self):
        before = self.path.read_bytes()
        with self.assertRaises(FileExistsError):
            ProjectStore.import_json(self.path, self.store.export_json())
        self.assertEqual(self.path.read_bytes(), before)

    def test_import_rejects_tampered_provenance_unknown_fields_and_order(self):
        self.review("accept")
        self.review("exclude")
        original = json.loads(self.store.export_json())
        mutations = [lambda data: data.update(schema_version=3),
                     lambda data: data.update(format="original-serialized-project"),
                     lambda data: data["project"].update(schema_version=True),
                     lambda data: data["analyses"][0].update(input_sha256="0" * 64),
                     lambda data: data["analyses"][0]["result"]["calls"][0].update(label="tampered"),
                     lambda data: data["analyses"][0].update(settings_sha256="0" * 64),
                     lambda data: data["samples"][0].update(extra="not in schema"),
                     lambda data: data["review_events"].reverse(),
                     lambda data: data["review_events"][0].update(sequence=99),
                     lambda data: data["review_events"][0].update(target_id="missing")]
        for index, mutate in enumerate(mutations):
            payload = json.loads(json.dumps(original))
            mutate(payload)
            path = self.root / f"invalid-{index}.sqlite"
            with self.assertRaises(ProjectStoreError):
                ProjectStore.import_json(path, json.dumps(payload))
            self.assertFalse(path.exists())

    def test_malformed_import_never_creates_destination(self):
        for index, document in enumerate(("{", "[]", '{"schema_version":1,"schema_version":1}',
                                          '{"format":"opentrace-local-project","schema_version":1}',
                                          "NaN")):
            destination = self.root / f"malformed-{index}.sqlite"
            with self.assertRaises(ProjectStoreError):
                ProjectStore.import_json(destination, document)
            self.assertFalse(destination.exists())

    def test_unsupported_database_version_is_rejected(self):
        incompatible = self.root / "future.sqlite"
        with ProjectStore.import_json(incompatible, self.store.export_json()):
            pass
        connection = sqlite3.connect(incompatible)
        try:
            connection.execute("PRAGMA user_version=999")
            connection.commit()
        finally:
            connection.close()
        with self.assertRaisesRegex(ProjectStoreError, "version"):
            ProjectStore.open(incompatible)

    def test_competing_undo_transactions_allow_only_one_reversal(self):
        event = self.review("accept")
        barrier = Barrier(2)

        def reverse(actor):
            with ProjectStore.open(self.path) as store:
                barrier.wait(timeout=10)
                try:
                    store.undo_review(self.analysis, event, actor=actor, reason="Concurrent review")
                    return "written"
                except ProjectStoreError:
                    return "already-reversed"

        with ThreadPoolExecutor(max_workers=2) as executor:
            outcomes = list(executor.map(reverse, ("one", "two")))
        self.assertEqual(sorted(outcomes), ["already-reversed", "written"])
        self.assertEqual(len(self.store.review_events()), 2)
        self.assertIsNone(self.overlay()["accepted"])

    def test_maximum_result_depth_survives_export_and_reopen(self):
        nested = None
        for _ in range(32):
            nested = {"n": nested}
        identifier = self.add_analysis(result=nested)
        document = self.store.export_json()
        with ProjectStore.import_json(self.root / "deep.sqlite", document) as imported:
            self.assertEqual(imported.get_analysis(identifier)["result"], nested)

    def test_open_rejects_corrupted_json_and_releases_file_handle(self):
        corrupted = self.root / "corrupted.sqlite"
        with ProjectStore.import_json(corrupted, self.store.export_json()):
            pass
        connection = sqlite3.connect(corrupted)
        try:
            connection.execute("DROP TRIGGER immutable_analysis_records_update")
            connection.execute("UPDATE analysis_records SET result_json=?", ("{",))
            connection.commit()
        finally:
            connection.close()
        with self.assertRaises(ProjectStoreError):
            ProjectStore.open(corrupted)
        # Windows refuses this operation if an error path leaked its connection.
        moved = self.root / "corrupted-inspected.sqlite"
        corrupted.rename(moved)
        self.assertTrue(moved.exists())


if __name__ == "__main__":
    unittest.main()
