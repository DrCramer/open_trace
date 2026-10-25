"""Bounds-checked reader of the public ABIF v1 directory and data types.

No vendor software or algorithms are used. Unknown data stays opaque.
"""
from dataclasses import dataclass
from pathlib import Path
import struct

MAX_FILE_BYTES = 64 * 1024 * 1024
MAX_ENTRIES = 20000
ENTRY = struct.Struct(">4siHHIIII")
NUMERIC = {1: ("B", 1), 3: ("H", 2), 4: ("h", 2),
           5: ("i", 4), 7: ("f", 4), 8: ("d", 8), 13: ("B", 1)}


class AbifError(ValueError):
    """Malformed or unsupported ABIF input."""


@dataclass(frozen=True)
class Tag:
    name: str
    number: int
    element_type: int
    element_size: int
    count: int
    size: int
    offset: int
    data: bytes

    @property
    def key(self):
        return f"{self.name}{self.number}"

    def value(self):
        """Return decoded standard data; unknown types remain bytes."""
        if self.element_type in NUMERIC:
            code, width = NUMERIC[self.element_type]
            if self.element_size != width or self.size != self.count * width:
                raise AbifError(f"{self.key}: numeric count/size mismatch")
            result = struct.unpack(f">{self.count}{code}", self.data)
            return result[0] if self.count == 1 else list(result)
        if self.element_type in (2, 18, 19):
            if self.element_size != 1 or self.count != self.size:
                raise AbifError(f"{self.key}: string count/size mismatch")
        if self.element_type == 2:
            return self.data.decode("ascii", "replace").rstrip("\x00")
        if self.element_type == 18:
            if not self.data or self.data[0] + 1 != self.size:
                raise AbifError(f"{self.key}: truncated Pascal string")
            return self.data[1:1 + self.data[0]].decode("ascii", "replace")
        if self.element_type == 19:
            if not self.data or self.data[-1] != 0:
                raise AbifError(f"{self.key}: unterminated C string")
            return self.data[:-1].decode("ascii", "replace")
        if self.element_type in (10, 11):
            if self.element_size != 4 or self.size != self.count * 4:
                raise AbifError(f"{self.key}: date/time size mismatch")
            fmt = ">HBB" if self.element_type == 10 else ">BBBB"
            values = [list(struct.unpack_from(fmt, self.data, i * 4))
                      for i in range(self.count)]
            return values[0] if self.count == 1 else values
        return self.data


class AbifFile:
    def __init__(self, data: bytes, filename="sample.fsa"):
        if len(data) > MAX_FILE_BYTES:
            raise AbifError("File exceeds 64 MiB limit")
        if len(data) < 128 or data[:4] != b"ABIF":
            raise AbifError("Expected ABIF signature and 128-byte header")
        self.filename = Path(filename).name
        self.version = struct.unpack_from(">H", data, 4)[0]
        if self.version // 100 != 1:
            raise AbifError(f"Unsupported ABIF version {self.version}; this reader supports v1. "
                            "HID/ABIF v3 requires a separate documented decoder.")
        # The public specification requires readers to ignore other root fields.
        root = ENTRY.unpack_from(data, 6)
        count, directory = root[4], root[6]
        if count > MAX_ENTRIES:
            raise AbifError("Too many directory entries")
        self._check_range(data, directory, count * ENTRY.size, "directory")
        self.tags = {}
        for i in range(count):
            pos = directory + i * ENTRY.size
            raw_name, num, kind, width, n, size, offset, _ = ENTRY.unpack_from(data, pos)
            if any(c < 0x21 or c > 0x7e for c in raw_name):
                raise AbifError(f"Invalid ASCII tag name at entry {i}")
            name = raw_name.decode("ascii")
            key = (name, num)
            if key in self.tags:
                raise AbifError(f"Duplicate tag {name}{num}")
            if size <= 4:
                payload = data[pos + 20:pos + 20 + size]
            else:
                self._check_range(data, offset, size, f"{name}{num}")
                payload = data[offset:offset + size]
            tag = Tag(name, num, kind, width, n, size, offset, payload)
            if kind in NUMERIC or kind in (2, 10, 11, 18, 19):
                tag.value()  # Validate known payloads before exposing the file.
            self.tags[key] = tag

    @staticmethod
    def _check_range(data, offset, size, label):
        if offset < 128 or offset > len(data) or size > len(data) - offset:
            raise AbifError(f"{label}: range outside file")

    @classmethod
    def read(cls, path):
        path = Path(path)
        if path.stat().st_size > MAX_FILE_BYTES:
            raise AbifError("File exceeds 64 MiB limit")
        return cls(path.read_bytes(), path.name)

    def value(self, name, number=1, default=None):
        tag = self.tags.get((name, number))
        return tag.value() if tag else default

    def text(self, name, number=1, default=""):
        value = self.value(name, number)
        return value if isinstance(value, str) else default

    def channels(self):
        """Expose numeric DATA arrays, with explicit inferred dye mapping.

        DATA1..4 + DATA105/106 are the six raw traces observed in supplied
        GlobalFiler Express FSA data. Do not map DATA5..8 auxiliaries to dyes.
        Analyzed DATA9..12 + DATA205/206 are retained as a separate group.
        """
        channels = []
        fragment2 = self.text("SVER", 4).strip().lower() == "fragment format 2.0"
        edr = fragment2 and self.value("EDRF", 1) == 1
        for (name, number), tag in sorted(self.tags.items()):
            if name != "DATA" or tag.element_type not in NUMERIC:
                continue
            value = tag.value()
            values = value if isinstance(value, list) else [value]
            if not values:
                continue
            if fragment2:
                # ID-X IfaAbifReader.getAllRawElectropherograms: in "Fragment Format 2.0" files values that do not
                # fit a short are stored sparsely (OvrV/OvrI per DATA number; EDRV/EDRI when EDRF = 1)
                values = list(values)
                for vt, it in (("OvrV", "OvrI"), ("EDRV", "EDRI")) if edr else (("OvrV", "OvrI"),):
                    ov, oi = self.value(vt, number), self.value(it, number)
                    if ov is None and oi is None:
                        continue
                    ov = ov if isinstance(ov, list) else [ov]
                    oi = oi if isinstance(oi, list) else [oi]
                    if len(ov) != len(oi):
                        raise AbifError(f"{tag.key}: {vt}/{it} have different sizes of values and indices")
                    for v, i in zip(ov, oi):
                        if not isinstance(i, int) or not 0 <= i < len(values):
                            raise AbifError(f"{tag.key}: {it} contains invalid indices")
                        values[i] = v
            dye_index = None
            group = "auxiliary"
            if number in (1, 2, 3, 4, 105, 106):
                dye_index = number if number <= 4 else number - 100
                group = "raw"
            elif number in (9, 10, 11, 12, 205, 206):
                dye_index = number - 8 if number <= 12 else number - 200
                group = "analyzed"
            channels.append({"tag": tag.key, "number": number, "group": group,
                             "dye_index": dye_index,
                             "dye": self.text("DyeN", dye_index) if dye_index else "",
                             "wavelength_nm": self.value("DyeW", dye_index) if dye_index and
                             isinstance(self.value("DyeW", dye_index), int) else None,
                             "values": values})
        # ID-X: dye traces of different lengths are all cut to the shortest one (no padding)
        raw = [ch for ch in channels if ch["group"] == "raw"]
        if raw:
            n = min(len(ch["values"]) for ch in raw)
            for ch in raw:
                if len(ch["values"]) > n:
                    ch["values"] = ch["values"][:n]
                    ch["truncated_to"] = n
        return channels

    def summary(self):
        unsupported = [tag.key for tag in self.tags.values()
                       if tag.name == "DATA" and tag.element_type not in NUMERIC]
        return {"filename": self.filename, "version": self.version,
                "sample": self.text("SMPL", default=self.text("SpNm")),
                "instrument": self.text("MODL"),
                "dye_count": self.value("Dye#") if isinstance(self.value("Dye#"), int) else None,
                "tag_count": len(self.tags),
                "warnings": (["Unsupported opaque DATA tags: " + ", ".join(unsupported)]
                             if unsupported else []),
                "channels": [{k: v for k, v in ch.items() if k != "values"}
                             | {"points": len(ch["values"])} for ch in self.channels()]}

    def run_info(self):
        """Run/sample identification shown in the Samples table (all optional)."""
        def first(*names):
            for name in names:
                value = self.value(name)
                if isinstance(value, str) and value.strip():
                    return value.strip()
            return None
        date, time = self.value("RUND"), self.value("RUNT")
        stamp = None
        if isinstance(date, (list, tuple)) and len(date) >= 3:
            stamp = "%04d-%02d-%02d" % tuple(date[:3])
            if isinstance(time, (list, tuple)) and len(time) >= 3:
                stamp += " %02d:%02d:%02d" % tuple(time[:3])
        lane = self.value("LANE")
        return {"sample_name": first("SpNm", "SMPL"), "run_name": first("RunN"),
                "instrument_type": self.value("HCFG", 3) if isinstance(self.value("HCFG", 3), str) else first("MODL"),
                "instrument_id": first("MCHN"), "run_date": stamp, "well": first("TUBE"),
                "plate": first("CTNM", "CTID"), "capillary": lane if isinstance(lane, int) else None,
                "comment": first("CMNT")}

    def directory(self):
        return [{"tag": tag.key, "type": tag.element_type,
                 "count": tag.count, "bytes": tag.size,
                 "inline": tag.size <= 4, "offset": tag.offset}
                for tag in self.tags.values()]
