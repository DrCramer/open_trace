"""Original synthetic six-channel ABIF data, free of vendor sample data."""
import math
import struct
from .abif import ENTRY

SYNTHETIC_STANDARD_BP = (75, 105, 155, 205, 275, 335, 425, 505, 605)
SYNTHETIC_PIPELINE_STANDARD_BP = (40, 70, 110, 145, 195, 260)


def demo_bytes():
    items = []
    def add(name, number, kind, width, count, payload):
        items.append((name.encode("ascii"), number, kind, width, count, payload))
    add("SMPL", 1, 18, 1, 15, b"\x0eSynthetic demo")
    add("Dye#", 1, 4, 2, 1, struct.pack(">h", 6))
    for channel, number in enumerate((1, 2, 3, 4, 105, 106), 1):
        name = f"Dye {channel}".encode("ascii")
        add("DyeN", channel, 18, 1, len(name) + 1, bytes([len(name)]) + name)
        values = []
        for scan in range(3000):
            signal = 15 + 3 * math.sin(scan * .17 + channel)
            centers = (350 + 70 * channel, 950 + 40 * channel, 1700 + 50 * channel, 2350 + 20 * channel)
            for peak_index, center in enumerate(centers):
                signal += (300 + 70 * channel + peak_index * 90) * math.exp(-.5 * ((scan - center) / (5 + channel)) ** 2)
            values.append(round(signal))
        add("DATA", number, 4, 2, len(values), struct.pack(f">{len(values)}h", *values))
    return build_abif(items)


def sizing_demo_bytes():
    """Original nonlinear standard trace with two unrelated extra peaks.

    These invented fragments are not a commercial kit definition.
    The independently known generative curve is scan=200+3.2*bp+.0015*bp².
    """
    from .abif import AbifFile
    original = AbifFile(demo_bytes())
    label = b"Synthetic sizing"
    items = [(b"SMPL", 1, 18, 1, len(label) + 1, bytes([len(label)]) + label)]
    items.extend((tag.name.encode("ascii"), tag.number, tag.element_type,
                  tag.element_size, tag.count, tag.data)
                 for tag in original.tags.values()
                 if tag.name != "SMPL" and (tag.name, tag.number) != ("DATA", 106))
    centers = [(200 + 3.2 * bp + .0015 * bp * bp, 700 + i * 21, 5)
               for i, bp in enumerate(SYNTHETIC_STANDARD_BP)]
    centers.extend([(120, 1200, 5), (2125, 1500, 5)])
    values = []
    for scan in range(3000):
        signal = 15 + 2 * math.sin(scan * .13)
        signal += sum(height * math.exp(-.5 * ((scan - center) / width) ** 2)
                      for center, height, width in centers)
        values.append(round(signal))
    items.append((b"DATA", 106, 4, 2, len(values), struct.pack(">3000h", *values)))
    return build_abif(items)


def pipeline_demo_bytes():
    """Original slow-migration fixture for morphology/ratio/base-quality review.

    Invented standard: scan=200+20*bp, sigma5. Other dyes include known180bp
    peaks at scan3800. No manufacturer definitions or user trace data.
    """
    label = b'Synthetic morphology sizing'
    items = [(b'SMPL', 1, 18, 1, len(label)+1, bytes([len(label)])+label),
             (b'Dye#', 1, 4, 2, 1, struct.pack('>h',6))]
    for channel, number in enumerate((1,2,3,4,105,106),1):
        name = f'Dye {channel}'.encode('ascii')
        items.append((b'DyeN',channel,18,1,len(name)+1,bytes([len(name)])+name))
        centers = [200+20*bp for bp in SYNTHETIC_PIPELINE_STANDARD_BP] if channel == 6 else [1400+channel*15,2400,3800,4800-channel*10]
        values = [round(20+0.001*i+sum((1000 if channel == 6 else 300+channel*60+j*70)
                         *math.exp(-0.5*((i-center)/5)**2) for j,center in enumerate(centers)))
                  for i in range(6501)]
        items.append((b'DATA',number,4,2,len(values),struct.pack(f'>{len(values)}h',*values)))
    return build_abif(items)


def build_abif(items):
    """Small fixture writer for standard tags, used only by demo and tests."""
    directory = bytearray()
    payloads = bytearray()
    data_offset = 128 + len(items) * 28
    for name, number, kind, width, count, payload in items:
        if len(payload) <= 4:
            offset = int.from_bytes(payload.ljust(4, b"\x00"), "big")
        else:
            offset = data_offset + len(payloads)
            payloads.extend(payload)
        directory.extend(ENTRY.pack(name, number, kind, width, count, len(payload), offset, 0))
    root = ENTRY.pack(b"tdir", 1, 1023, 28, len(items), len(directory), 128, 0)
    header = (b"ABIF" + struct.pack(">H", 101) + root).ljust(128, b"\x00")
    return header + directory + payloads
