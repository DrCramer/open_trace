"""Plain CSV exports; sample labels are deliberately excluded from cells."""
import csv
import io


def traces_csv(channels):
    stream = io.StringIO(newline="")
    writer = csv.writer(stream)
    writer.writerow(["scan"] + [ch["tag"] for ch in channels])
    for scan in range(max((len(ch["values"]) for ch in channels), default=0)):
        writer.writerow([scan] + [ch["values"][scan] if scan < len(ch["values"]) else ""
                                  for ch in channels])
    return stream.getvalue()


def peaks_csv(channels):
    stream = io.StringIO(newline="")
    writer = csv.writer(stream)
    writer.writerow(["channel", "scan", "height", "raw_height", "area", "width_scans", "size_bp"])
    for ch in channels:
        for peak in ch.get("peaks", []):
            writer.writerow([ch["tag"]] + [peak.get(k, "") for k in
                            ("scan", "height", "raw_height", "area", "width_scans", "size_bp")])
    return stream.getvalue()
