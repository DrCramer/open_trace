"""Persistent independent run revisions, transaction boundaries and legacy replay."""

from concurrent.futures import ThreadPoolExecutor
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import sqlite3
import tempfile
from threading import Barrier
import unittest
from unittest.mock import patch

from genemapper_open.project_store import (
    MAX_RUN_INPUT_BYTES, ProjectStore, ProjectStoreError,
)


class ProjectStoreRunTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.path = Path(self.temporary.name) / "runs.sqlite"
        self.store = ProjectStore.create(self.path, "Run revisions")
        self.source = "a" * 64
        self.sample = self.store.add_sample(self.source, {"kind": "synthetic"})
        self.run = self.store.add_run("Explicit run", run_id="run-one")

    def tearDown(self):
        self.store.close()
        self.temporary.cleanup()

    def analysis(self, *, store=None, sample=None, source=None):
        target = store or self.store
        return target.add_analysis(sample or self.sample, source or self.source,
            {"name": "independent synthetic", "version": "test"}, {},
            {"calls": [{"id": "call-one", "marker": "M", "label": "A"}]}, "tentative")

    def inputs(self, keys=("sample-one",)):
        return {"samples": [{"sample_key": key, "analysis": {"source_sha256": self.source},
                             "opaque": {"precomputed": [True, None, 1.25]}} for key in keys],
                "explicit_policy": "independent recalculation"}

    def revision(self, *, parent=None, outputs=None, inputs=None, **kwargs):
        if outputs is None:
            previous = (self.store.get_run_revision(parent)["outputs"][0]["analysis_id"]
                        if parent else None)
            outputs = [{"sample_key": "sample-one", "analysis_id": self.analysis(),
                        "previous_analysis_id": previous}]
        return self.store.append_run_revision(self.run, self.inputs() if inputs is None else inputs,
            outputs, parent_revision_id=parent, actor="Researcher", reason="Explicit revision", **kwargs)

    def test_schema2_linear_revisions_roundtrip_and_detached_json(self):
        self.assertEqual(self.store.project["schema_version"], 2)
        first = self.revision(revision_id="revision-one")
        original = self.store.get_run_revision(first)
        second = self.revision(parent=first, revision_id="revision-two")
        self.assertEqual([r["revision_id"] for r in self.store.list_run_revisions(self.run)], [first, second])
        self.assertEqual(self.store.list_runs(), [self.store.get_run(self.run)])
        self.assertEqual(self.store.get_run_revision(first), original)
        for field in ("inputs", "outputs"):
            canonical = json.dumps(original[field], sort_keys=True, separators=(",", ":"), ensure_ascii=False)
            self.assertEqual(original[field + "_sha256"], hashlib.sha256(canonical.encode()).hexdigest())
        detached = self.store.get_run_revision(first)
        detached["inputs"]["samples"].clear()
        self.assertEqual(self.store.get_run_revision(first), original)
        exported = self.store.export_json()
        with ProjectStore.from_json(exported) as imported:
            self.assertEqual(imported.export_json(), exported)
        with ProjectStore.open(self.path) as reopened:
            self.assertEqual(reopened.export_json(), exported)

    def test_atomic_rollback_covers_samples_analyses_runs_reviews_and_revisions(self):
        before = self.store.export_json()
        with self.assertRaisesRegex(ProjectStoreError, "latest"):
            with self.store.atomic():
                sample = self.store.add_sample("b" * 64, {})
                analysis = self.analysis(sample=sample, source="b" * 64)
                self.store.append_review(analysis, "note", actor="r", reason="context", value="note")
                self.store.add_run("Transient")
                root = self.revision()
                self.revision(parent=None)
        self.assertEqual(self.store.export_json(), before)
        self.assertFalse(self.store._connection.in_transaction)
        with self.store.atomic():
            root = self.revision()
            self.assertEqual(len(json.loads(self.store.export_json())["run_revisions"]), 1)
        self.assertEqual(self.store.get_run_revision(root)["sequence"], 1)

    def test_nested_atomic_savepoint_can_rollback_independently(self):
        with self.store.atomic():
            keep = self.analysis()
            with self.assertRaises(ValueError):
                with self.store.atomic():
                    discard = self.analysis()
                    self.revision()
                    raise ValueError("rollback inner")
            self.assertEqual(self.store.list_run_revisions(), [])
            self.store.get_analysis(keep)
            with self.assertRaises(ProjectStoreError):
                self.store.get_analysis(discard)
            self.revision(outputs=[{"sample_key": "sample-one", "analysis_id": keep,
                                    "previous_analysis_id": None}])
        with ProjectStore.open(self.path) as reopened:
            self.assertEqual(len(reopened.list_analyses()), 1)
            self.assertEqual(len(reopened.list_run_revisions()), 1)

    def test_export_inside_atomic_sees_pending_writes_without_committing_them(self):
        before = self.store.export_json()
        observer = sqlite3.connect(self.path)
        try:
            with self.assertRaisesRegex(RuntimeError, "after export"):
                with self.store.atomic():
                    root = self.revision()
                    pending = json.loads(self.store.export_json())
                    self.assertEqual(pending["run_revisions"][0]["revision_id"], root)
                    self.assertEqual(len(pending["analyses"]), 1)
                    self.assertTrue(self.store._connection.in_transaction)
                    self.assertEqual(observer.execute("SELECT count(*) FROM run_revisions").fetchone()[0], 0)
                    self.assertEqual(observer.execute("SELECT count(*) FROM analysis_records").fetchone()[0], 0)
                    raise RuntimeError("after export")
            self.assertEqual(self.store.export_json(), before)
            self.assertFalse(self.store._connection.in_transaction)
            self.assertEqual(observer.execute("SELECT count(*) FROM run_revisions").fetchone()[0], 0)
        finally:
            observer.close()

    def test_whole_project_export_bound_failure_rolls_back_atomic_revision(self):
        before = self.store.export_json()
        limit = len(before.encode("utf-8")) + 100
        with self.assertRaisesRegex(ProjectStoreError, "byte limit"):
            with self.store.atomic():
                self.revision()
                with patch("genemapper_open.project_store.MAX_EXPORT_BYTES", limit):
                    self.store.export_json()
        self.assertEqual(self.store.export_json(), before)
        self.assertFalse(self.store._connection.in_transaction)

    def test_reference_sources_unique_outputs_and_exact_input_coverage(self):
        analysis = self.analysis()
        base = [{"sample_key": "sample-one", "analysis_id": analysis, "previous_analysis_id": None}]
        cases = [
            (self.inputs(), [{**base[0], "analysis_id": "unknown"}]),
            (self.inputs(), [{**base[0], "previous_analysis_id": analysis}]),
            (self.inputs(), [{**base[0], "extra": True}]),
            (self.inputs(), [base[0], base[0]]),
            (self.inputs(("sample-one", "sample-two")),
             [base[0], {**base[0], "sample_key": "sample-two"}]),
            (self.inputs(("sample-one", "sample-two")), base),
            (self.inputs(("sample-one", "sample-one")), base),
            ({"samples": [{"sample_key": "sample-one", "analysis": {"source_sha256": "b" * 64}}]}, base),
        ]
        for inputs, outputs in cases:
            with self.subTest(inputs=inputs, outputs=outputs):
                before = self.store.export_json()
                with self.assertRaises(ProjectStoreError):
                    self.revision(inputs=inputs, outputs=outputs)
                self.assertEqual(self.store.export_json(), before)
        root = self.revision(outputs=base)
        with self.assertRaisesRegex(ProjectStoreError, "new analysis"):
            self.revision(parent=root, outputs=[{**base[0], "previous_analysis_id": analysis}])
        wrong_previous = [{**base[0], "analysis_id": self.analysis(), "previous_analysis_id": None}]
        with self.assertRaisesRegex(ProjectStoreError, "parent sample"):
            self.revision(parent=root, outputs=wrong_previous)

    def test_removed_and_new_keys_link_only_to_direct_parent_outputs(self):
        first = self.revision()
        old_analysis = self.store.get_run_revision(first)["outputs"][0]["analysis_id"]
        second = self.revision(parent=first, inputs=self.inputs(("new-sample",)), outputs=[{
            "sample_key": "new-sample", "analysis_id": self.analysis(), "previous_analysis_id": None}])
        new_analysis = self.analysis()
        with self.assertRaisesRegex(ProjectStoreError, "parent sample"):
            self.revision(parent=second, outputs=[{"sample_key": "sample-one", "analysis_id": new_analysis,
                                                  "previous_analysis_id": old_analysis}])
        self.revision(parent=second, outputs=[{"sample_key": "sample-one", "analysis_id": new_analysis,
                                               "previous_analysis_id": None}])

    def test_run_contexts_show_current_superseded_removed_without_changing_review(self):
        first = self.revision()
        old = self.store.get_run_revision(first)["outputs"][0]["analysis_id"]
        baseline = self.store.effective_analysis(old)
        self.assertEqual(baseline["run_contexts"][0]["freshness"], "current")
        second = self.revision(parent=first)
        new = self.store.get_run_revision(second)["outputs"][0]["analysis_id"]
        expected = [{"run_id": self.run, "revision_id": first, "sample_key": "sample-one",
                     "latest_revision_id": second, "latest_analysis_id": new, "freshness": "superseded"}]
        self.assertEqual(self.store.analysis_run_contexts(old), expected)
        stale = self.store.effective_analysis(old)
        self.assertEqual(stale["machine_result"], baseline["machine_result"])
        self.assertEqual(stale["machine_status"], baseline["machine_status"])
        self.assertEqual(stale["review_quality"], baseline["review_quality"])
        third = self.revision(parent=second, inputs=self.inputs(()), outputs=[])
        context = self.store.analysis_run_contexts(old)[0]
        self.assertEqual(context["freshness"], "removed")
        self.assertEqual(context["latest_revision_id"], third)
        self.assertIsNone(context["latest_analysis_id"])
        self.assertEqual(self.store.analysis_run_contexts(self.analysis()), [])

    def test_stale_roots_and_parents_reject_without_changes(self):
        root = self.revision()
        newer = self.revision(parent=root)
        for parent in (None, root, "unknown"):
            with self.subTest(parent=parent):
                output = [{"sample_key": "sample-one", "analysis_id": self.analysis(),
                           "previous_analysis_id": None}]
                before = self.store.export_json()
                with self.assertRaisesRegex(ProjectStoreError, "latest"):
                    self.revision(parent=parent, outputs=output)
                self.assertEqual(self.store.export_json(), before)
        other = self.store.add_run("Other run")
        with self.assertRaisesRegex(ProjectStoreError, "latest"):
            self.store.append_run_revision(other, self.inputs(()), [], parent_revision_id=newer,
                                           actor="r", reason="cross run")

    def test_sql_uniqueness_and_immutable_triggers(self):
        root = self.revision()
        child = self.revision(parent=root)
        for revision in (root, child):
            row = dict(self.store._connection.execute(
                "SELECT * FROM run_revisions WHERE revision_id=?", (revision,)).fetchone())
            row.pop("sequence")
            row["revision_id"] = "duplicate-" + revision
            with self.assertRaises(sqlite3.IntegrityError):
                with self.store.atomic():
                    self.store._connection.execute(
                        "INSERT INTO run_revisions (" + ",".join(row) + ") VALUES (" + ",".join("?" for _ in row) + ")",
                        tuple(row.values()))
        for table in ("runs", "run_revisions"):
            for operation in (f"DELETE FROM {table}", f"UPDATE {table} SET rowid=rowid"):
                with self.assertRaises(sqlite3.IntegrityError):
                    with self.store.atomic():
                        self.store._connection.execute(operation)

    def test_concurrent_revision_loser_rolls_back_its_dependent_analysis(self):
        root = self.revision()
        previous = self.store.get_run_revision(root)["outputs"][0]["analysis_id"]
        barrier = Barrier(2)

        def append(actor):
            with ProjectStore.open(self.path) as store:
                barrier.wait(timeout=10)
                try:
                    with store.atomic():
                        analysis = self.analysis(store=store)
                        return store.append_run_revision(self.run, self.inputs(), [{"sample_key": "sample-one",
                            "analysis_id": analysis, "previous_analysis_id": previous}],
                            parent_revision_id=root, actor=actor, reason="competing write")
                except ProjectStoreError as exc:
                    return str(exc)

        with ThreadPoolExecutor(max_workers=2) as executor:
            results = list(executor.map(append, ("one", "two")))
        self.assertEqual(sum("latest run revision" in result for result in results), 1)
        self.assertEqual(len(self.store.list_analyses()), 2)
        self.assertEqual(len(self.store.list_run_revisions()), 2)

    def test_inputs_allow_more_than_analysis_limit_but_enforce_finite_json_and_16mib(self):
        self.assertEqual(MAX_RUN_INPUT_BYTES, 16 * 1024 * 1024)
        large = self.inputs(())
        large["opaque"] = "x" * (3 * 1024 * 1024)
        root = self.revision(inputs=large, outputs=[])
        self.assertEqual(len(self.store.get_run_revision(root)["inputs"]["opaque"]), 3 * 1024 * 1024)
        for invalid in (float("nan"), float("inf"), {1: "non-string-key"}):
            inputs = self.inputs(())
            inputs["opaque"] = invalid
            with self.assertRaises(ProjectStoreError):
                self.revision(parent=root, inputs=inputs, outputs=[])
        with patch("genemapper_open.project_store.MAX_RUN_INPUT_BYTES", 64):
            with self.assertRaisesRegex(ProjectStoreError, "64 byte"):
                self.revision(parent=root, inputs=large, outputs=[])

    def test_strict_v2_replay_rejects_tampered_hashes_sequence_refs_and_unknown_fields(self):
        root = self.revision()
        self.revision(parent=root)
        document = json.loads(self.store.export_json())
        mutations = [
            lambda d: d.update(schema_version=3),
            lambda d: d["project"].update(schema_version=1),
            lambda d: d["run_revisions"][0].update(inputs_sha256="0" * 64),
            lambda d: d["run_revisions"][0].update(outputs_sha256="0" * 64),
            lambda d: d["run_revisions"][0]["inputs"].update(policy="tampered"),
            lambda d: d["run_revisions"][0]["outputs"][0].update(analysis_id="unknown"),
            lambda d: d["run_revisions"][1]["outputs"][0].update(previous_analysis_id=None),
            lambda d: d["run_revisions"][1].update(parent_revision_id=None),
            lambda d: d["run_revisions"][1].update(sequence=1),
            lambda d: d["run_revisions"].reverse(),
            lambda d: d["runs"][0].update(extra="unknown"),
        ]
        for mutate in mutations:
            candidate = deepcopy(document)
            mutate(candidate)
            with self.subTest(mutation=mutate):
                with self.assertRaises(ProjectStoreError):
                    ProjectStore.from_json(json.dumps(candidate))

    def test_legacy_json_and_sqlite_stay_v1_until_explicit_independent_upgrade(self):
        legacy_path = Path(self.temporary.name) / "legacy.sqlite"
        with ProjectStore._memory("Legacy", "legacy-project", "2026-10-21T00:00:00Z", schema_version=1) as legacy:
            sample = legacy.add_sample(self.source, {}, sample_id="legacy-sample")
            analysis = legacy.add_analysis(sample, self.source, {"name": "old", "version": "1"}, {},
                {"calls": [{"id": "legacy-call", "marker": "M", "label": "A"}]}, "tentative",
                analysis_id="legacy-analysis")
            legacy.append_review(analysis, "change-label", actor="r", reason="review", target_kind="call",
                                 target_id="legacy-call", value="B", event_id="legacy-event")
            exported = legacy.export_json()
            with legacy._save_new(legacy_path):
                pass
        before = legacy_path.read_bytes()
        with ProjectStore.open(legacy_path) as legacy:
            self.assertEqual(legacy.export_json(), exported)
            self.assertEqual(legacy.list_runs(), [])
            self.assertNotIn("run_contexts", legacy.effective_analysis(analysis))
            with self.assertRaisesRegex(ProjectStoreError, "upgrade"):
                legacy.add_run("Cannot implicitly upgrade")
            with legacy.upgraded() as upgraded:
                converted = json.loads(upgraded.export_json())
                original = json.loads(exported)
                self.assertEqual(converted["schema_version"], 2)
                for field in ("samples", "analyses", "review_events"):
                    self.assertEqual(converted[field], original[field])
                self.assertEqual(upgraded.project["project_id"], legacy.project["project_id"])
                upgraded.add_run("New independent run")
                self.assertEqual(legacy.export_json(), exported)
        self.assertEqual(legacy_path.read_bytes(), before)
        with ProjectStore.from_json(exported) as imported:
            self.assertEqual(imported.export_json(), exported)
        with ProjectStore.import_json(Path(self.temporary.name) / "legacy-copy.sqlite", exported) as imported:
            self.assertEqual(imported.export_json(), exported)


if __name__ == "__main__":
    unittest.main()
