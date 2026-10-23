"""Independent, local research project storage and append-only review history.

One explicit SQLite file contains one project.  This is a local audit trail,
not an authenticated user system or an electronic-signature implementation.
Only source digests and metadata are stored; callers retain their trace files.
Analysis JSON may contain top-level ``calls`` and ``peaks`` arrays whose items
have unique string ``id`` values. Those identifiers address review overlays;
the stored machine result is never rewritten by a review.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
import sqlite3
from contextlib import contextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Any
from uuid import uuid4

SCHEMA_VERSION = 2
SUPPORTED_SCHEMA_VERSIONS = (1, 2)
EXPORT_FORMAT = "opentrace-local-project"
MAX_JSON_BYTES = 2 * 1024 * 1024
MAX_EXPORT_BYTES = 64 * 1024 * 1024
MAX_RUN_INPUT_BYTES = 16 * 1024 * 1024
_SHA = re.compile(r"[0-9a-f]{64}\Z")
_ACTIONS = {"accept", "exclude", "change-label", "note", "undo", "add-allele", "delete-artifact-label",
            "override-gq", "override-cgq"}
_PEAK_EDITS = {"add-allele", "delete-artifact-label"}
_OVERRIDES = {"override-gq", "override-cgq"}
# OverrideGQCommand forces GQ through n(1.0); SampleQualityMetrics.s ignores
# its argument and writes 0.5, the renderer's GreenOverridePass category.
_OVERRIDE_GQ = 1.0
_OVERRIDE_CGQ = 0.5
_ARTIFACT_FLAGS = ("pullup", "spike")
# Own explicit policy, compared with the static original command bodies.
EDIT_POLICY = {
    "add_allele": "user_call_on_marker_linked_peak_clears_pullup_and_spike",
    "add_allele_undo": "full_event_reversal_restores_all_machine_flags",
    "original_add_undo_restores_spike": False,
    "omr_flag_modeled": False,
    "delete_artifact_label": "hide_label_keep_flags",
    "delete_artifact_label_undoable": False,
    "override_gq": "marker_gq_forced_to_1.0_marker_stale_sample_freshness_unchanged",
    "override_cgq": "sample_cgq_forced_to_0.5_override_pass_category",
    "ladder_override_run_effects_modeled": False,
    "numeric_quality_recomputed": False,
}


def _gq_rows(result: dict) -> dict[str, dict]:
    """Machine marker GQ rows by marker name; the override targets."""
    quality = result.get("quality")
    gq = quality.get("marker_gq") if isinstance(quality, dict) else None
    rows = gq.get("markers") if isinstance(gq, dict) else None
    return {row["marker"]: row for row in rows if isinstance(row, dict) and isinstance(row.get("marker"), str)} \
        if isinstance(rows, list) else {}


class ProjectStoreError(ValueError):
    """Invalid input, provenance, schema, or review reference."""


def _text(value: Any, name: str, limit: int = 1024) -> str:
    if not isinstance(value, str) or not value.strip() or len(value) > limit:
        raise ProjectStoreError(f"{name} must be nonempty text of at most {limit} characters")
    return value


def _sha(value: Any) -> str:
    if not isinstance(value, str) or not _SHA.fullmatch(value):
        raise ProjectStoreError("SHA-256 must be 64 lowercase hexadecimal characters")
    return value


def _timestamp(value: Any = None) -> str:
    if value is None:
        return datetime.now(timezone.utc).isoformat(timespec="microseconds")
    _text(value, "timestamp", 64)
    try:
        parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError as exc:
        raise ProjectStoreError("timestamp must be ISO 8601 with timezone") from exc
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise ProjectStoreError("timestamp must include a timezone")
    return value


def _canonical(value: Any, limit: int = MAX_JSON_BYTES, *, depth_limit: int = 32) -> str:
    def check(item: Any, depth: int = 0) -> None:
        if depth > depth_limit:
            raise ProjectStoreError("JSON exceeds maximum nesting depth")
        if item is None or type(item) in (str, bool, int):
            return
        if type(item) is float and math.isfinite(item):
            return
        if type(item) is list:
            for child in item:
                check(child, depth + 1)
            return
        if type(item) is dict and all(type(key) is str for key in item):
            for child in item.values():
                check(child, depth + 1)
            return
        raise ProjectStoreError("value must contain only finite JSON values and string keys")

    check(value)
    try:
        result = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
        size = len(result.encode("utf-8"))
    except (ValueError, UnicodeError, OverflowError) as exc:
        raise ProjectStoreError("invalid JSON value") from exc
    if size > limit:
        raise ProjectStoreError(f"JSON exceeds {limit} byte limit")
    return result


def _object(value: Any, name: str) -> dict:
    if type(value) is not dict:
        raise ProjectStoreError(f"{name} must be a JSON object")
    return value


def _digest(canonical: str) -> str:
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _targets(result: dict) -> dict[tuple[str, str], dict]:
    targets = {}
    for kind, plural in (("call", "calls"), ("peak", "peaks")):
        records = result.get(plural, [])
        if type(records) is not list:
            raise ProjectStoreError(f"result.{plural} must be an array")
        for item in records:
            _object(item, f"result.{plural} item")
            identifier = _text(item.get("id"), f"{kind} id", 256)
            key = (kind, identifier)
            if key in targets:
                raise ProjectStoreError(f"duplicate {kind} id: {identifier}")
            targets[key] = item
    return targets


def _peak_flag(peak: dict, name: str) -> bool:
    flags = peak.get("flags", [])
    listed = (flags.get(name) is True if isinstance(flags, dict)
              else name in flags if isinstance(flags, list) else False)
    return peak.get(name) is True or listed


def _peak_contexts(result: dict) -> dict[str, dict]:
    """Marker link, machine artifact flags and artifact label of every peak.

    Only links recorded by allele calling associate a peak with a marker; the
    size-standard dye never appears in result peaks. A pull-up flag carries the
    full-setter label ``SPU`` unless an explicit non-blank label was retained.
    """
    peaks = result.get("peaks", [])
    contexts = {}
    for peak in peaks:
        classification = peak.get("pullup_classification")
        state = classification.get("state") if isinstance(classification, dict) else None
        label = state.get("artifact_label") if isinstance(state, dict) else None
        flags = {name: _peak_flag(peak, name) for name in _ARTIFACT_FLAGS}
        if not flags["pullup"]:
            label = None
        elif not (isinstance(label, str) and label.strip()):
            label = "SPU"
        contexts[peak["id"]] = {"marker": None, "artifact_flags": flags,
                                "artifact_label": label, "machine_call_ids": []}
    calling = result.get("allele_calling")
    markers = calling.get("markers") if isinstance(calling, dict) else None
    for marker in markers if isinstance(markers, list) else []:
        if not isinstance(marker, dict) or not isinstance(marker.get("name"), str):
            continue
        links = marker.get("peak_review")
        for entry in links if isinstance(links, list) else []:
            index = entry.get("peak_index") if isinstance(entry, dict) else None
            if type(index) is int and 0 <= index < len(peaks):
                contexts[peaks[index]["id"]]["marker"] = marker["name"]
    for call in result.get("calls", []):
        source = call.get("source_peak_id")
        if isinstance(source, str) and source in contexts:
            contexts[source]["machine_call_ids"].append(call["id"])
    return contexts


def _review_quality(result: dict, overlays: dict, events: list[dict]) -> dict:
    """Describe freshness against the final reviewed call interpretation.

    This independent policy leaves machine measurements untouched. A call's
    exclusion, or a changed label on an included call, invalidates its marker
    and the sample summary. Superseded events, labels on excluded calls and
    changes back to the machine interpretation have no additional effect.
    ``unchanged`` preserves the baseline; it does not claim available quality.
    """
    latest = {}
    for event in events:
        if event["target_kind"] == "call":
            field = ("accepted" if event["action"] in {"accept", "exclude"}
                     else "label" if event["action"] == "change-label" else None)
            if field:
                latest[(event["target_id"], field)] = event["event_id"]
    affected = {}
    active_ids = set()
    call_markers = set()
    for call in result.get("calls", []):
        marker = call.get("marker")
        marker = marker if isinstance(marker, str) and marker.strip() else None
        if marker is not None:
            call_markers.add(marker)
        overlay = overlays[("call", call["id"])]
        event_id = None
        if overlay["accepted"] is False:
            event_id = latest[(call["id"], "accepted")]
        elif overlay["label"] != call.get("label"):
            event_id = latest[(call["id"], "label")]
        if event_id is not None:
            active_ids.add(event_id)
            if marker is not None:
                affected.setdefault(marker, set()).add(event_id)
    # Peak edits invalidate like the original PlotEditCommand: the linked
    # marker and the sample become stale; nothing numeric is recomputed.
    contexts = _peak_contexts(result)
    pullup_cleared = []
    gq_overrides = {}
    cgq_override = None
    for event in events:
        if event["action"] == "override-gq":
            # The original invalidation receives a null Sample: only the
            # marker becomes stale, the sample stale state is unchanged.
            gq_overrides[event["target_id"]] = event["event_id"]
            call_markers.add(event["target_id"])
            affected.setdefault(event["target_id"], set()).add(event["event_id"])
            continue
        if event["action"] == "override-cgq":
            cgq_override = event["event_id"]
            continue
        if event["target_kind"] != "peak" or event["action"] not in _PEAK_EDITS:
            continue
        context = contexts[event["target_id"]]
        active_ids.add(event["event_id"])
        if context["marker"] is not None:
            call_markers.add(context["marker"])
            affected.setdefault(context["marker"], set()).add(event["event_id"])
        if event["action"] == "add-allele" and context["artifact_flags"]["pullup"]:
            pullup_cleared.append(event["event_id"])
    quality = result.get("quality")
    gq = quality.get("marker_gq") if isinstance(quality, dict) else None
    gq = gq if isinstance(gq, dict) else {}
    machine_status = gq.get("status") if isinstance(gq.get("status"), str) else None
    rows = gq.get("markers")
    marker_status = {}
    for row in rows if isinstance(rows, list) else []:
        if isinstance(row, dict) and isinstance(row.get("marker"), str) and row["marker"].strip():
            marker_status[row["marker"]] = row.get("status") if isinstance(row.get("status"), str) else None
    override_ids = set(gq_overrides.values())
    ordered_ids = [event["event_id"] for event in events
                   if event["event_id"] in active_ids or event["event_id"] in override_ids]
    output = {
        "policy": "net_call_interpretation",
        "affected_markers": sorted(affected),
        "active_event_ids": ordered_ids,
        "markers": [
            {"marker": marker,
             "freshness": "stale" if marker in affected or machine_status == "stale"
                          or marker_status.get(marker) == "stale" else "unchanged",
             "machine_status": marker_status.get(marker, machine_status),
             "active_event_ids": [identifier for identifier in ordered_ids
                                  if identifier in affected.get(marker, set())]}
            for marker in sorted(call_markers | marker_status.keys())
        ],
        "sample_summary": {
            "freshness": "stale" if active_ids or machine_status == "stale"
                         or "stale" in marker_status.values() else "unchanged",
            "machine_status": machine_status,
        },
    }
    # Override entries appear only while active, keeping documents without
    # overrides byte-identical to earlier releases.
    rows_by_marker = _gq_rows(result)
    for row in output["markers"]:
        if row["marker"] in gq_overrides:
            score = (rows_by_marker.get(row["marker"]) or {}).get("score")
            machine_gq = score.get("gq") if isinstance(score, dict) else None
            row["gq_override"] = {"gq": _OVERRIDE_GQ, "override": True,
                                  "event_id": gq_overrides[row["marker"]], "machine_gq": machine_gq}
    if cgq_override is not None:
        output["sample_summary"]["cgq_override"] = {
            "cgq": _OVERRIDE_CGQ, "category": "override_pass", "override": True, "event_id": cgq_override,
            "machine_category": gq.get("sample_category")}
    # Present only for results carrying a machine pull-up category, so older
    # reports keep their exact review-quality document.
    if isinstance(quality, dict) and isinstance(quality.get("pullup"), dict):
        output["pullup_profile"] = {"freshness": "stale" if pullup_cleared else "unchanged",
                                    "active_event_ids": pullup_cleared}
    return output


_DDL = """
CREATE TABLE store_info (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    schema_version INTEGER NOT NULL, project_id TEXT NOT NULL,
    name TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE sample_records (
    sample_id TEXT PRIMARY KEY, source_sha256 TEXT NOT NULL,
    metadata_json TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE analysis_records (
    analysis_id TEXT PRIMARY KEY,
    sample_id TEXT NOT NULL REFERENCES sample_records(sample_id),
    input_sha256 TEXT NOT NULL, algorithm_json TEXT NOT NULL,
    algorithm_sha256 TEXT NOT NULL, settings_json TEXT NOT NULL,
    settings_sha256 TEXT NOT NULL, result_json TEXT NOT NULL,
    result_sha256 TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE review_log (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT UNIQUE NOT NULL,
    analysis_id TEXT NOT NULL REFERENCES analysis_records(analysis_id),
    action TEXT NOT NULL, target_kind TEXT NOT NULL, target_id TEXT,
    actor TEXT NOT NULL, reason TEXT NOT NULL, created_at TEXT NOT NULL,
    value_json TEXT NOT NULL,
    reverses_event_id TEXT REFERENCES review_log(event_id)
);
CREATE TRIGGER analysis_source_binding BEFORE INSERT ON analysis_records
WHEN NEW.input_sha256 != (SELECT source_sha256 FROM sample_records WHERE sample_id=NEW.sample_id)
BEGIN SELECT RAISE(ABORT, 'analysis input hash differs from sample'); END;
"""


_RUN_DDL = """
CREATE TABLE runs (
    run_id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE run_revisions (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    revision_id TEXT UNIQUE NOT NULL,
    run_id TEXT NOT NULL REFERENCES runs(run_id),
    parent_revision_id TEXT,
    actor TEXT NOT NULL, reason TEXT NOT NULL, created_at TEXT NOT NULL,
    inputs_json TEXT NOT NULL, inputs_sha256 TEXT NOT NULL,
    outputs_json TEXT NOT NULL, outputs_sha256 TEXT NOT NULL,
    UNIQUE(run_id, revision_id),
    FOREIGN KEY(run_id, parent_revision_id) REFERENCES run_revisions(run_id, revision_id)
);
CREATE UNIQUE INDEX run_single_root ON run_revisions(run_id) WHERE parent_revision_id IS NULL;
CREATE UNIQUE INDEX run_single_child ON run_revisions(parent_revision_id) WHERE parent_revision_id IS NOT NULL;
"""


class ProjectStore:
    """Use :meth:`create` or :meth:`open` with an explicit filesystem path."""

    def __init__(self, connection: sqlite3.Connection, path: Path | None):
        self._connection = connection
        self.path = path
        self._atomic_depth = 0
        self._savepoint_counter = 0
        connection.row_factory = sqlite3.Row
        connection.execute("PRAGMA foreign_keys=ON")

    @classmethod
    def memory(cls, name: str) -> ProjectStore:
        """Create a transient browser project; export explicitly to retain it."""
        return cls._memory(name, str(uuid4()), _timestamp())

    @classmethod
    def from_json(cls, document: str) -> ProjectStore:
        """Validate/replay a portable project entirely in memory, without files."""
        return cls._replay_json(document)

    @classmethod
    def _memory(cls, name: str, project_id: str, created_at: str,
                schema_version: int = SCHEMA_VERSION) -> ProjectStore:
        if type(schema_version) is not int or schema_version not in SUPPORTED_SCHEMA_VERSIONS:
            raise ProjectStoreError("unsupported project schema version")
        store = cls(sqlite3.connect(":memory:"), None)
        store._connection.executescript(_DDL)
        if schema_version == 2:
            store._connection.executescript(_RUN_DDL)
        store._connection.execute(
            "INSERT INTO store_info VALUES (1, ?, ?, ?, ?)",
            (schema_version, _text(project_id, "project_id", 256), _text(name, "name"), _timestamp(created_at)),
        )
        tables = ("store_info", "sample_records", "analysis_records", "review_log")
        for table in tables + (("runs", "run_revisions") if schema_version == 2 else ()):
            # Identifiers are fixed internal constants, never caller-provided SQL.
            for operation in ("UPDATE", "DELETE"):
                store._connection.execute(
                    f"CREATE TRIGGER immutable_{table}_{operation.lower()} BEFORE {operation} ON {table} "
                    "BEGIN SELECT RAISE(ABORT, 'immutable project record'); END"
                )
        store._connection.execute(f"PRAGMA user_version={schema_version}")
        store._connection.commit()
        return store

    @classmethod
    def create(cls, path: str | Path, name: str) -> ProjectStore:
        """Create a new project; an existing destination is always refused."""
        with cls._memory(name, str(uuid4()), _timestamp()) as memory:
            return memory._save_new(path)

    def _save_new(self, path: str | Path) -> ProjectStore:
        destination = Path(path).absolute()
        if str(path) == ":memory:":
            raise ProjectStoreError("an explicit filesystem destination is required")
        # Exclusive creation prevents replacing an existing project, including an empty file.
        with destination.open("xb"):
            pass
        connection = None
        try:
            connection = sqlite3.connect(destination)
            self._connection.backup(connection)
            connection.close()
            return self.open(destination)
        except BaseException:
            if connection is not None:
                connection.close()
            destination.unlink(missing_ok=True)
            raise

    @classmethod
    def open(cls, path: str | Path) -> ProjectStore:
        """Open an existing schema exactly; never discover or upgrade files."""
        destination = Path(path).absolute()
        if not destination.is_file():
            raise ProjectStoreError("project file does not exist")
        connection = sqlite3.connect(destination.as_uri() + "?mode=rw", uri=True)
        store = cls(connection, destination)
        try:
            version = connection.execute("PRAGMA user_version").fetchone()[0]
            info = connection.execute("SELECT * FROM store_info").fetchall()
            if version not in SUPPORTED_SCHEMA_VERSIONS or len(info) != 1 or info[0]["schema_version"] != version:
                raise ProjectStoreError("unsupported project schema version")
            if connection.execute("PRAGMA quick_check").fetchone()[0] != "ok":
                raise ProjectStoreError("invalid SQLite project")
            if connection.execute("PRAGMA foreign_key_check").fetchone():
                raise ProjectStoreError("project contains broken references")
            # Replay in memory to validate digests, input binding and event references.
            with cls._replay_json(store.export_json()):
                pass
            return store
        except (sqlite3.DatabaseError, ValueError, TypeError, KeyError, RecursionError) as exc:
            connection.close()
            if isinstance(exc, ProjectStoreError):
                raise
            raise ProjectStoreError("not a supported SQLite project") from exc

    def close(self) -> None:
        self._connection.close()

    def __enter__(self) -> ProjectStore:
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    @contextmanager
    def atomic(self):
        """Commit a group together; nested groups roll back to a savepoint."""
        outermost = self._atomic_depth == 0
        self._savepoint_counter += 1
        savepoint = f"project_atomic_{self._savepoint_counter}"
        self._connection.execute("BEGIN IMMEDIATE" if outermost else f"SAVEPOINT {savepoint}")
        self._atomic_depth += 1
        try:
            yield self
        except BaseException:
            if outermost:
                self._connection.rollback()
            else:
                self._connection.execute(f"ROLLBACK TO {savepoint}")
                self._connection.execute(f"RELEASE {savepoint}")
            raise
        else:
            try:
                if outermost:
                    self._connection.commit()
                else:
                    self._connection.execute(f"RELEASE {savepoint}")
            except BaseException:
                if outermost:
                    self._connection.rollback()
                else:
                    self._connection.execute(f"ROLLBACK TO {savepoint}")
                    self._connection.execute(f"RELEASE {savepoint}")
                raise
        finally:
            self._atomic_depth -= 1

    @contextmanager
    def _write(self):
        if self._atomic_depth:
            yield
        else:
            with self.atomic():
                yield

    @contextmanager
    def _read(self):
        """Reuse an active transaction without owning its BEGIN or commit."""
        if self._connection.in_transaction:
            yield
        else:
            with self._connection:
                self._connection.execute("BEGIN")
                yield

    def upgraded(self) -> ProjectStore:
        """Return an independent schema2 memory copy without changing this store."""
        document = json.loads(self.export_json())
        document["schema_version"] = SCHEMA_VERSION
        document["project"]["schema_version"] = SCHEMA_VERSION
        document.setdefault("runs", [])
        document.setdefault("run_revisions", [])
        return self.from_json(_canonical(document, MAX_EXPORT_BYTES, depth_limit=40))

    @property
    def project(self) -> dict:
        row = dict(self._connection.execute("SELECT * FROM store_info WHERE singleton=1").fetchone())
        row.pop("singleton")
        return row

    def add_sample(self, source_sha256: str, metadata: dict, *, sample_id: str | None = None,
                   created_at: str | None = None) -> str:
        identifier = _text(str(uuid4()) if sample_id is None else sample_id, "sample_id", 256)
        _object(metadata, "metadata")
        if any(type(value) in (dict, list) for value in metadata.values()):
            raise ProjectStoreError("sample metadata contains scalar fields only; traces are not stored")
        payload = _canonical(_object(metadata, "metadata"), 65536)
        with self._write():
            self._connection.execute(
                "INSERT INTO sample_records VALUES (?, ?, ?, ?)",
                (identifier, _sha(source_sha256), payload, _timestamp(created_at)),
            )
        return identifier

    def list_samples(self) -> list[dict]:
        return [dict(sample_id=row["sample_id"], source_sha256=row["source_sha256"],
                     metadata=json.loads(row["metadata_json"]), created_at=row["created_at"])
                for row in self._connection.execute("SELECT * FROM sample_records ORDER BY sample_id")]

    def add_analysis(self, sample_id: str, input_sha256: str, algorithm: dict, settings: dict,
                     result: dict, status: str, *, analysis_id: str | None = None,
                     created_at: str | None = None) -> str:
        """Append an immutable result, with exact algorithm/settings/result digests.

        ``algorithm`` requires explicit ``name`` and ``version``. ``status`` is an
        explicit machine status, not inferred from review or claimed as validation.
        """
        identifier = _text(str(uuid4()) if analysis_id is None else analysis_id, "analysis_id", 256)
        _text(sample_id, "sample_id", 256)
        _sha(input_sha256)
        _object(algorithm, "algorithm")
        _text(algorithm.get("name"), "algorithm.name", 256)
        _text(algorithm.get("version"), "algorithm.version", 256)
        algorithm_json = _canonical(algorithm, 65536)
        settings_json = _canonical(_object(settings, "settings"), 262144)
        result_json = _canonical(_object(result, "result"))
        _targets(result)
        _text(status, "status", 128)
        source = self._connection.execute(
            "SELECT source_sha256 FROM sample_records WHERE sample_id=?", (sample_id,)
        ).fetchone()
        if source is None or source[0] != input_sha256:
            raise ProjectStoreError("analysis input SHA-256 must match the registered sample")
        with self._write():
            self._connection.execute(
                "INSERT INTO analysis_records VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (identifier, sample_id, input_sha256, algorithm_json, _digest(algorithm_json),
                 settings_json, _digest(settings_json), result_json, _digest(result_json),
                 status, _timestamp(created_at)),
            )
        return identifier

    def get_analysis(self, analysis_id: str) -> dict:
        _text(analysis_id, "analysis_id", 256)
        row = self._connection.execute(
            "SELECT * FROM analysis_records WHERE analysis_id=?", (analysis_id,)
        ).fetchone()
        if row is None:
            raise ProjectStoreError("unknown analysis_id")
        value = dict(row)
        for field in ("algorithm", "settings", "result"):
            value[field] = json.loads(value.pop(field + "_json"))
            if _digest(_canonical(value[field])) != value[field + "_sha256"]:
                raise ProjectStoreError(f"stored {field} digest mismatch")
        return value

    def list_analyses(self) -> list[dict]:
        return [self.get_analysis(row[0]) for row in self._connection.execute(
            "SELECT analysis_id FROM analysis_records ORDER BY analysis_id")]

    def _require_runs(self) -> None:
        if self.project["schema_version"] != 2:
            raise ProjectStoreError("run history requires an explicit schema2 upgrade")

    def add_run(self, name: str, *, run_id: str | None = None,
                created_at: str | None = None) -> str:
        self._require_runs()
        identifier = _text(str(uuid4()) if run_id is None else run_id, "run_id", 256)
        with self._write():
            self._connection.execute("INSERT INTO runs VALUES (?, ?, ?)",
                                     (identifier, _text(name, "run name"), _timestamp(created_at)))
        return identifier

    def list_runs(self) -> list[dict]:
        if self.project["schema_version"] == 1:
            return []
        return [dict(row) for row in self._connection.execute("SELECT * FROM runs ORDER BY run_id")]

    def get_run(self, run_id: str) -> dict:
        self._require_runs()
        row = self._connection.execute("SELECT * FROM runs WHERE run_id=?",
                                       (_text(run_id, "run_id", 256),)).fetchone()
        if row is None:
            raise ProjectStoreError("unknown run_id")
        return dict(row)

    def get_run_revision(self, revision_id: str) -> dict:
        self._require_runs()
        row = self._connection.execute("SELECT * FROM run_revisions WHERE revision_id=?",
                                       (_text(revision_id, "revision_id", 256),)).fetchone()
        if row is None:
            raise ProjectStoreError("unknown revision_id")
        value = dict(row)
        for field, limit in (("inputs", MAX_RUN_INPUT_BYTES), ("outputs", MAX_JSON_BYTES)):
            value[field] = json.loads(value.pop(field + "_json"))
            if _digest(_canonical(value[field], limit)) != value[field + "_sha256"]:
                raise ProjectStoreError(f"stored run {field} digest mismatch")
        return value

    def list_run_revisions(self, run_id: str | None = None) -> list[dict]:
        if self.project["schema_version"] == 1:
            if run_id is not None:
                self._require_runs()
            return []
        if run_id is None:
            rows = self._connection.execute("SELECT revision_id FROM run_revisions ORDER BY sequence")
        else:
            self.get_run(run_id)
            rows = self._connection.execute(
                "SELECT revision_id FROM run_revisions WHERE run_id=? ORDER BY sequence", (run_id,))
        return [self.get_run_revision(row[0]) for row in rows]

    def append_run_revision(self, run_id: str, inputs: dict, outputs: list[dict], *,
                            parent_revision_id: str | None, actor: str, reason: str,
                            revision_id: str | None = None, created_at: str | None = None) -> str:
        """Append one linear revision binding fresh analyses to exact run inputs.

        Input fields beyond samples' keys/source hashes are opaque finite JSON.
        Outputs cover exactly the input sample keys and never reuse an analysis
        already referenced by a run revision. New sample keys have no previous
        analysis; existing keys must reference the direct parent's output.
        """
        self._require_runs()
        identifier = _text(str(uuid4()) if revision_id is None else revision_id, "revision_id", 256)
        _text(run_id, "run_id", 256)
        _text(actor, "actor", 256)
        _text(reason, "reason", 4096)
        timestamp = _timestamp(created_at)
        if parent_revision_id is not None:
            _text(parent_revision_id, "parent_revision_id", 256)
        inputs_json = _canonical(_object(inputs, "run inputs"), MAX_RUN_INPUT_BYTES)
        if type(inputs.get("samples")) is not list or type(outputs) is not list:
            raise ProjectStoreError("run inputs.samples and outputs must be arrays")
        sources = {}
        for sample in inputs["samples"]:
            _object(sample, "run input sample")
            key = _text(sample.get("sample_key"), "sample_key", 256)
            if key in sources:
                raise ProjectStoreError("duplicate input sample_key")
            analysis = _object(sample.get("analysis"), "run input sample.analysis")
            sources[key] = _sha(analysis.get("source_sha256"))
        outputs_json = _canonical(outputs)
        output_keys, output_ids = set(), set()
        for output in outputs:
            _object(output, "run output")
            if set(output) != {"sample_key", "analysis_id", "previous_analysis_id"}:
                raise ProjectStoreError("run output requires sample_key, analysis_id and previous_analysis_id")
            key = _text(output["sample_key"], "sample_key", 256)
            analysis_id = _text(output["analysis_id"], "analysis_id", 256)
            previous = output["previous_analysis_id"]
            if previous is not None:
                _text(previous, "previous_analysis_id", 256)
            if key in output_keys or analysis_id in output_ids:
                raise ProjectStoreError("duplicate output sample_key or analysis_id")
            output_keys.add(key)
            output_ids.add(analysis_id)
        if output_keys != sources.keys():
            raise ProjectStoreError("run outputs must cover exactly the input sample keys")
        with self._write():
            self.get_run(run_id)
            latest = self._connection.execute(
                "SELECT revision_id FROM run_revisions WHERE run_id=? ORDER BY sequence DESC LIMIT 1",
                (run_id,)).fetchone()
            if parent_revision_id != (latest[0] if latest else None):
                raise ProjectStoreError("parent_revision_id must match the latest run revision")
            previous_outputs = ({item["sample_key"]: item["analysis_id"] for item in
                                 self.get_run_revision(parent_revision_id)["outputs"]}
                                if parent_revision_id is not None else {})
            referenced_ids = {item["analysis_id"] for revision in self.list_run_revisions()
                              for item in revision["outputs"]}
            if output_ids & referenced_ids:
                raise ProjectStoreError("run outputs require new analysis records")
            for output in outputs:
                key = output["sample_key"]
                if output["previous_analysis_id"] != previous_outputs.get(key):
                    raise ProjectStoreError("previous_analysis_id must match the parent sample output")
                analysis = self.get_analysis(output["analysis_id"])
                if analysis["input_sha256"] != sources[key]:
                    raise ProjectStoreError("run input source SHA-256 must match its output analysis")
            self._connection.execute(
                "INSERT INTO run_revisions (revision_id,run_id,parent_revision_id,actor,reason,created_at,"
                "inputs_json,inputs_sha256,outputs_json,outputs_sha256) VALUES (?,?,?,?,?,?,?,?,?,?)",
                (identifier, run_id, parent_revision_id, actor, reason, timestamp,
                 inputs_json, _digest(inputs_json), outputs_json, _digest(outputs_json)))
        return identifier

    def analysis_run_contexts(self, analysis_id: str) -> list[dict]:
        """Locate immutable analysis outputs relative to each run's latest revision."""
        self.get_analysis(analysis_id)
        revisions = self.list_run_revisions()
        latest = {revision["run_id"]: revision for revision in revisions}
        contexts = []
        for revision in revisions:
            for output in revision["outputs"]:
                if output["analysis_id"] != analysis_id:
                    continue
                current = latest[revision["run_id"]]
                current_id = next((item["analysis_id"] for item in current["outputs"]
                                   if item["sample_key"] == output["sample_key"]), None)
                contexts.append({
                    "run_id": revision["run_id"], "revision_id": revision["revision_id"],
                    "sample_key": output["sample_key"], "latest_revision_id": current["revision_id"],
                    "latest_analysis_id": current_id,
                    "freshness": "removed" if current_id is None else
                                 "current" if current_id == analysis_id else "superseded",
                })
        return contexts

    def review_events(self, analysis_id: str | None = None) -> list[dict]:
        if analysis_id is None:
            rows = self._connection.execute("SELECT * FROM review_log ORDER BY sequence")
        else:
            self.get_analysis(analysis_id)
            rows = self._connection.execute(
                "SELECT * FROM review_log WHERE analysis_id=? ORDER BY sequence", (analysis_id,))
        output = []
        for row in rows:
            event = dict(row)
            event["value"] = json.loads(event.pop("value_json"))
            output.append(event)
        return output

    def append_review(self, analysis_id: str, action: str, *, actor: str, reason: str,
                      target_kind: str = "analysis", target_id: str | None = None,
                      value: str | None = None, event_id: str | None = None,
                      created_at: str | None = None, reverses_event_id: str | None = None) -> str:
        """Append review without modifying the machine result or earlier events.

        accept/exclude/change-label address a call or peak; note can also address
        the entire analysis. Undo is one new event reversing one non-undo event.
        Sequence numbers, rather than client timestamps, define event order.
        """
        identifier = _text(str(uuid4()) if event_id is None else event_id, "event_id", 256)
        _text(actor, "actor", 256)
        _text(reason, "reason", 4096)
        _text(action, "action", 128)
        _text(target_kind, "target_kind", 128)
        timestamp = _timestamp(created_at)
        if action not in _ACTIONS:
            raise ProjectStoreError("unsupported review action")
        analysis = self.get_analysis(analysis_id)
        targets = _targets(analysis["result"])
        if target_kind == "analysis":
            if target_id is not None or action not in {"note", "undo", "override-cgq"}:
                raise ProjectStoreError("only notes and CGQ overrides may directly target an analysis")
        elif target_kind == "marker":
            _text(target_id, "target_id", 256)
            # Stored undo events carry the reversed event's target, so a
            # replayed import also presents undo against a marker.
            if action not in {"override-gq", "undo"}:
                raise ProjectStoreError("marker targets accept only override-gq")
            if target_id not in _gq_rows(analysis["result"]):
                raise ProjectStoreError("override-gq requires a marker with a machine GQ row")
        else:
            _text(target_id, "target_id", 256)
            if target_kind not in {"call", "peak"} or (target_kind, target_id) not in targets:
                raise ProjectStoreError("unknown review target")
        if action == "override-gq" and target_kind != "marker":
            raise ProjectStoreError("override-gq requires a marker target")
        if action == "override-cgq":
            if target_kind != "analysis":
                raise ProjectStoreError("override-cgq targets the whole analysis")
            quality = analysis["result"].get("quality")
            if not isinstance(quality, dict) or not isinstance(quality.get("marker_gq"), dict):
                raise ProjectStoreError("override-cgq requires machine marker GQ results")
        if action in {"change-label", "note"}:
            _text(value, "review value", 4096)
        elif action == "add-allele":
            # The original dialog rejects null, empty and longer than 256.
            _text(value, "allele label", 256)
        elif value is not None:
            raise ProjectStoreError("this review action does not take a value")
        if action in _PEAK_EDITS:
            if target_kind != "peak":
                raise ProjectStoreError(f"{action} requires a peak target")
            context = _peak_contexts(analysis["result"])[target_id]
            if action == "add-allele":
                if context["marker"] is None:
                    raise ProjectStoreError("add-allele requires a peak linked to a marker window")
                if context["machine_call_ids"]:
                    raise ProjectStoreError("peak already has a machine call; review that call instead")
            elif context["artifact_label"] is None:
                raise ProjectStoreError("delete-artifact-label requires a peak with a machine artifact label")
        with self._write():
            if action in _OVERRIDES:
                rows = self._connection.execute(
                    "SELECT event_id, action, target_kind, target_id, reverses_event_id "
                    "FROM review_log WHERE analysis_id=?", (analysis_id,)).fetchall()
                reversed_ids = {row["reverses_event_id"] for row in rows if row["action"] == "undo"}
                if any(row["action"] == action and row["target_kind"] == target_kind
                       and row["target_id"] == target_id and row["event_id"] not in reversed_ids for row in rows):
                    raise ProjectStoreError(f"{action} is already in effect for this target")
            if action in _PEAK_EDITS:
                rows = self._connection.execute(
                    "SELECT event_id, action, target_kind, target_id, reverses_event_id "
                    "FROM review_log WHERE analysis_id=?", (analysis_id,)).fetchall()
                reversed_ids = {row["reverses_event_id"] for row in rows if row["action"] == "undo"}
                active = {row["action"] for row in rows if row["action"] in _PEAK_EDITS
                          and row["target_kind"] == "peak" and row["target_id"] == target_id
                          and row["event_id"] not in reversed_ids}
                if action in active or (action == "delete-artifact-label" and "add-allele" in active):
                    raise ProjectStoreError(f"{action} is already in effect for this peak")
            # Serialize validation and insertion so concurrent undo attempts cannot
            # both reverse the same event. Other appends also receive stable order.
            if action == "undo":
                event = self._connection.execute(
                    "SELECT * FROM review_log WHERE event_id=?", (reverses_event_id,)
                ).fetchone()
                if event is None or event["analysis_id"] != analysis_id or event["action"] == "undo":
                    raise ProjectStoreError("undo requires an earlier non-undo event in this analysis")
                if event["action"] == "delete-artifact-label":
                    raise ProjectStoreError("artifact label deletion is not undoable, as in the original command")
                if self._connection.execute(
                    "SELECT 1 FROM review_log WHERE reverses_event_id=?", (reverses_event_id,)
                ).fetchone():
                    raise ProjectStoreError("event is already reversed")
                target_kind, target_id = event["target_kind"], event["target_id"]
            elif reverses_event_id is not None:
                raise ProjectStoreError("only undo may reference a reversed event")
            self._connection.execute(
                "INSERT INTO review_log (event_id, analysis_id, action, target_kind, target_id, "
                "actor, reason, created_at, value_json, reverses_event_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (identifier, analysis_id, action, target_kind, target_id, actor, reason,
                 timestamp, _canonical(value, 65536), reverses_event_id),
            )
        return identifier

    def undo_review(self, analysis_id: str, event_id: str, *, actor: str, reason: str) -> str:
        return self.append_review(analysis_id, "undo", actor=actor, reason=reason,
                                  reverses_event_id=event_id)

    def effective_analysis(self, analysis_id: str) -> dict:
        """Return machine result plus a separate, deterministically folded review."""
        analysis = self.get_analysis(analysis_id)
        events = self.review_events(analysis_id)
        reversed_ids = {event["reverses_event_id"] for event in events if event["action"] == "undo"}
        overlays = {}
        contexts = _peak_contexts(analysis["result"])
        for (kind, identifier), item in _targets(analysis["result"]).items():
            overlays[(kind, identifier)] = {
                "target_kind": kind, "target_id": identifier, "accepted": None,
                "label": item.get("label"), "notes": [], "event_ids": [],
            }
            if kind == "peak":
                context = contexts[identifier]
                overlays[(kind, identifier)].update(
                    marker=context["marker"], user_allele=None,
                    machine_artifact_flags=dict(context["artifact_flags"]),
                    artifact_flags=dict(context["artifact_flags"]),
                    machine_artifact_label=context["artifact_label"],
                    artifact_label=context["artifact_label"])
        notes = []
        active_events = []
        for event in events:
            if event["action"] == "undo" or event["event_id"] in reversed_ids:
                continue
            active_events.append(event)
            if event["action"] in _OVERRIDES:
                continue  # Folded into review_quality; no call/peak overlay or note.
            if event["target_kind"] == "analysis":
                notes.append({"event_id": event["event_id"], "text": event["value"]})
                continue
            overlay = overlays[(event["target_kind"], event["target_id"])]
            overlay["event_ids"].append(event["event_id"])
            if event["action"] in {"accept", "exclude"}:
                overlay["accepted"] = event["action"] == "accept"
            elif event["action"] == "change-label":
                overlay["label"] = event["value"]
            elif event["action"] == "note":
                overlay["notes"].append({"event_id": event["event_id"], "text": event["value"]})
            elif event["action"] == "add-allele":
                overlay["user_allele"] = {"label": event["value"], "call_type": "USER",
                                          "marker": overlay["marker"], "event_id": event["event_id"],
                                          "actor": event["actor"]}
                overlay["artifact_flags"] = {name: False for name in overlay["artifact_flags"]}
                overlay["artifact_label"] = None
            elif event["action"] == "delete-artifact-label":
                overlay["artifact_label"] = ""
        user_calls = [{"peak_id": overlay["target_id"], **overlay["user_allele"]}
                      for overlay in overlays.values() if overlay.get("user_allele")]
        effective = {"analysis_id": analysis_id, "machine_status": analysis["status"],
                "machine_result": analysis["result"], "review_targets": list(overlays.values()),
                "review_notes": notes, "review_event_count": len(events),
                "user_calls": user_calls, "edit_policy": dict(EDIT_POLICY),
                "review_quality": _review_quality(analysis["result"], overlays, active_events)}
        if self.project["schema_version"] == 2:
            effective["run_contexts"] = self.analysis_run_contexts(analysis_id)
        return effective

    def export_json(self) -> str:
        """Export canonical versioned JSON containing data, digests, and all events."""
        with self._read():
            version = self.project["schema_version"]
            document = {"format": EXPORT_FORMAT, "schema_version": version,
                        "project": self.project, "samples": self.list_samples(),
                        "analyses": self.list_analyses(), "review_events": self.review_events()}
            if version == 2:
                document.update(runs=self.list_runs(), run_revisions=self.list_run_revisions())
            # The wrapper adds depth around independently bounded result objects.
            return _canonical(document, MAX_EXPORT_BYTES, depth_limit=40)

    @classmethod
    def import_json(cls, path: str | Path, document: str) -> ProjectStore:
        """Validate and replay an export in memory, then create a new database.

        The complete canonical round trip must match, including hashes, sequence,
        identity and timestamps. Existing destinations are never overwritten.
        """
        with cls._replay_json(document) as store:
            return store._save_new(path)

    @classmethod
    def _replay_json(cls, document: str) -> ProjectStore:
        store = None
        def pairs(items: list[tuple[str, Any]]) -> dict:
            result = {}
            for key, value in items:
                if key in result:
                    raise ProjectStoreError("duplicate JSON object key")
                result[key] = value
            return result
        try:
            if not isinstance(document, str) or len(document.encode("utf-8")) > MAX_EXPORT_BYTES:
                raise ProjectStoreError("invalid or oversized project export")
            payload = json.loads(document, object_pairs_hook=pairs)
            _object(payload, "export")
            canonical = _canonical(payload, MAX_EXPORT_BYTES, depth_limit=40)
            version = payload.get("schema_version")
            if payload.get("format") != EXPORT_FORMAT or type(version) is not int or version not in SUPPORTED_SCHEMA_VERSIONS:
                raise ProjectStoreError("unsupported export format or schema version")
            project = _object(payload["project"], "project")
            if type(project.get("schema_version")) is not int or project["schema_version"] != version:
                raise ProjectStoreError("unsupported project schema version")
            for field in ("samples", "analyses", "review_events") + (("runs", "run_revisions") if version == 2 else ()):
                if type(payload[field]) is not list:
                    raise ProjectStoreError(f"{field} must be an array")
            store = cls._memory(project["name"], project["project_id"], project["created_at"], schema_version=version)
            for sample in payload["samples"]:
                store.add_sample(sample["source_sha256"], sample["metadata"],
                                 sample_id=sample["sample_id"], created_at=sample["created_at"])
            for analysis in payload["analyses"]:
                store.add_analysis(analysis["sample_id"], analysis["input_sha256"],
                                   analysis["algorithm"], analysis["settings"], analysis["result"],
                                   analysis["status"], analysis_id=analysis["analysis_id"],
                                   created_at=analysis["created_at"])
            for event in payload["review_events"]:
                store.append_review(event["analysis_id"], event["action"], actor=event["actor"],
                                    reason=event["reason"], target_kind=event["target_kind"],
                                    target_id=event["target_id"], value=event["value"],
                                    event_id=event["event_id"], created_at=event["created_at"],
                                    reverses_event_id=event["reverses_event_id"])
            if version == 2:
                for run in payload["runs"]:
                    store.add_run(run["name"], run_id=run["run_id"], created_at=run["created_at"])
                for revision in payload["run_revisions"]:
                    store.append_run_revision(revision["run_id"], revision["inputs"], revision["outputs"],
                        parent_revision_id=revision["parent_revision_id"], actor=revision["actor"],
                        reason=revision["reason"], revision_id=revision["revision_id"], created_at=revision["created_at"])
            if store.export_json() != canonical:
                raise ProjectStoreError("export round-trip mismatch (fields, order, hashes or sequence)")
            return store
        except (KeyError, TypeError, RecursionError, UnicodeError, json.JSONDecodeError, sqlite3.DatabaseError) as exc:
            if store is not None:
                store.close()
            raise ProjectStoreError("malformed project export") from exc
        except BaseException:
            if store is not None:
                store.close()
            raise
