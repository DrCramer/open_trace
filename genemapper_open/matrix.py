"""Dye matrices for 310 Genetic Analyzer data (GeneMapper ID-X Matrices tab, Matrix Editor, MNF).

Transcribed from the observed behaviour of GeneMapper ID-X 1.6 (GMp.jar):

* ``com.apldbio.ifa.Algorithm.MatrixMaker`` (``ai``) — matrix from matrix-standard runs:
  for every standard (one per dye) the selected data-point window of every raw filter trace is
  smoothed (7-point quadratic Savitzky-Golay, integer arithmetic), baselined with the GeneScan
  baseliner and, at data points where the standard's own dye trace is >= 100 RFU (at most 1000
  points, at least 10), the bleed-through slope into each other filter is estimated twice —
  least-absolute-deviation fit (Numerical Recipes ``medfit``/``rofunc``, last point unused) and a
  least-squares line through the origin point prepended — and the larger, floored at 0, is the
  matrix cell (row = filter, column = dye standard; diagonal 1). Cells are rounded to 4 decimals.
* ``com.apldbio.ifa.Algorithm.GeneScanBaseliner`` (``v``) — running means/minima, 21-point
  smoothing, two-sided tracking and subtraction.
* ``com.apldbio.ifa.BusinessLogic.Matrix`` — the raw matrix (``MRAW``) and its inverse
  (``MTRX`` = inverse x 1000 as int16, ``MTRD`` = dimension, optional ``CMNT``) written as an ABIF file
  (``IfaAbifWriter.writeMatrixToMatrixFile``, extension ``.mtx``).
* ``com.apldbio.ifa.Algorithm.MultiComponenter`` (``aj``) — dye signal = inverse x filter signal
  per data point, truncated toward zero (int16 wrap when the source trace is int16).

No vendor code is used; arithmetic is reproduced from the decompiled byte code.
"""
from __future__ import annotations

import math
import struct

from .abif import AbifFile, AbifError

MIN_POINTS = 200          # ai: a window shorter than 200 data points is rejected
MAX_PAIRS = 1000          # ai: at most 1000 data points per fit
MIN_PAIRS = 10            # ai: "does not have enough peaks"
PEAK_RFU = 100            # ai: data points used when the standard's own dye is >= 100
DYE_LETTERS = ('B', 'G', 'Y', 'R', 'O')


class MatrixError(ValueError):
    def __init__(self, code, message, filename=None):
        super().__init__(message)
        self.code = code
        self.filename = filename


def _jdiv(a, b):
    """Java integer division (truncates toward zero)."""
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


def _d2i(x):
    """Java (int) cast of a double."""
    if math.isnan(x):
        return 0
    return int(max(-2147483648, min(2147483647, math.trunc(x))))


def smooth7(values):
    """ai.a(int[],int,int): (-2,3,6,7,6,3,-2)/21 on indices 3..n-4; edges untouched."""
    n = len(values)
    out = list(values)
    for i in range(3, n - 3):
        s = (-2 * (values[i + 3] + values[i - 3]) + 3 * (values[i + 2] + values[i - 2])
             + 6 * (values[i + 1] + values[i - 1]) + 7 * values[i])
        out[i] = _jdiv(s, 21)
    return out


def _running(arr, w, clip):
    """v.a(int[],int,int): running mean of width w; with clip=1 the mean never exceeds the signal."""
    n = len(arr)
    if n < w:
        return
    tmp = [0] * n
    total = 0
    for i in range(w):
        tmp[i] = arr[i]
        total += arr[i]
    for i in range(n - w, n):
        tmp[i] = arr[i]
    h = w // 2
    tmp[h] = _jdiv(total, w)
    for i in range(h + 1, n - h):
        total += arr[i + h] - arr[i - h - 1]
        tmp[i] = _jdiv(total, w)
    if clip == 1:
        for i in range(n):
            if arr[i] < tmp[i]:
                tmp[i] = arr[i]
    for i in range(n):
        arr[i] = tmp[i] if tmp[i] >= 0 else 0


_SG21 = (329, 324, 309, 284, 249, 204, 149, 84, 9, -76, -171)


def _smooth21(arr):
    n = len(arr)
    if n < 21:
        return
    out = [0] * n
    for i in range(10, n - 10):
        s = _SG21[0] * arr[i]
        for k in range(1, 11):
            s += _SG21[k] * (arr[i + k] + arr[i - k])
        out[i] = _jdiv(s, 3059)
    for i in range(10, n - 10):
        arr[i] = out[i]


def _track(arr, direction):
    """v.a(int[],int): baseline follower from one end (direction -1 starts at the last point)."""
    n = len(arr)
    out = [0] * n
    if direction == -1:
        start, step = n - 1, -1
    else:
        start, step = 0, 1
    avg = 0.0
    i = start
    for _ in range(10):
        avg += arr[i]
        i += step
    avg /= 10.0
    i = start
    for _ in range(n):
        if arr[i] <= _d2i(avg):
            avg = float(arr[i])
            out[i] = arr[i]
        else:
            d = arr[i] - avg
            w = d
            if d > 20.0:
                w = d * d * d
            elif d > 10.0:
                w = d * d
            k = 1.0 / w if w >= 1.0 else 0.0
            avg = avg + k * d
            out[i] = _d2i(avg + 0.5)
        i += step
    return out


def genescan_baseline(signal):
    """GeneScanBaseliner.a(int[]): returns the baseline-subtracted signal (negative values -> 0)."""
    if signal is None:
        raise MatrixError('no_signal', 'No signal is available')
    b = [int(v) for v in signal]
    _running(b, 11, 0)
    _running(b, len(b) // 4, 1)
    for w in (21, 51, 101, 201):
        _running(b, w, 1)
    _smooth21(b)
    _running(b, 21, 1)
    rev = _track(b, -1)
    fwd = _track(b, 1)
    for i in range(len(b)):
        b[i] = rev[i] if rev[i] > fwd[i] else fwd[i]
    return [max(0, int(signal[i]) - b[i]) for i in range(len(b))]


def _rofunc(x, y, b):
    """ai.a(double[],double[],double): NR rofunc over the first n-1 points -> [sum, abdev, aa]."""
    n = len(x) - 1
    arr = sorted(y[j] - b * x[j] for j in range(n))
    lo = (n - 1) // 2
    hi = n - 1 - lo
    aa = 0.5 * (arr[lo] + arr[hi])
    total = 0.0
    abdev = 0.0
    for j in range(n):
        d = y[j] - (b * x[j] + aa)
        abdev += abs(d)
        total += x[j] if d > 0.0 else -x[j]
    return [total, abdev, aa]


def medfit(x, y):
    """ai.a(double[],double[]): NR medfit (least absolute deviation) -> [a, b, abdev/n]."""
    n = len(x) - 1
    sx = sy = sxy = sxx = 0.0
    for j in range(n):
        sx += x[j]
        sy += y[j]
        sxy += x[j] * y[j]
        sxx += x[j] * x[j]
    dl = n * sxx - sx * sx
    bb = (n * sxy - sx * sy) / dl
    aa = (sy - bb * sx) / n
    chisq = 0.0
    for j in range(n):
        t = y[j] - (aa + bb * x[j])
        chisq += t * t
    sigb = math.sqrt(chisq / dl)
    if sigb < 1e-5:
        sigb = 1e-5
    b1 = bb
    r = _rofunc(x, y, b1)
    f1 = r[0]
    step = abs(sigb * 3.0)
    b2 = bb + (step if f1 > 0.0 else -step)
    r = _rofunc(x, y, b2)
    f2 = r[0]
    it = 0
    while f1 * f2 > 0.0:
        bb = 2.0 * b2 - b1
        b1, f1, b2 = b2, f2, bb
        r = _rofunc(x, y, b2)
        f2 = r[0]
        it += 1
        if it > 1000:
            return [0.0, 0.0, 0.0]
    sigb *= 0.01
    while abs(b2 - b1) > sigb:
        bb = 0.5 * (b1 + b2)
        if bb == b1 or bb == b2:
            break
        r = _rofunc(x, y, bb)
        f = r[0]
        if f * f1 >= 0.0:
            f1 = f
            b1 = bb
        else:
            b2 = bb
    return [r[2], bb, r[1] / n]


def linear_fit(x, y):
    """ToolBox.a(double[],double[],1): least-squares straight line -> [a, b]."""
    n = len(x)
    sx = sum(x)
    sy = sum(y)
    sxx = sum(v * v for v in x)
    sxy = sum(x[j] * y[j] for j in range(n))
    dl = n * sxx - sx * sx
    if dl == 0:
        return [sy / n if n else 0.0, 0.0]
    b = (n * sxy - sx * sy) / dl
    return [(sy - b * sx) / n, b]


def round4(v):
    """ToolBox.a(double,4): Math.round(v*1e4)/1e4 (half up)."""
    return math.floor(v * 10000.0 + 0.5) / 10000.0


def raw_traces(fsa_bytes, filename='matrix.fsa', dyes=None):
    """Raw filter traces DATA1..DATAn of a sample file (n = dyes or every raw trace)."""
    a = AbifFile(fsa_bytes, filename)
    raw = {}
    for ch in a.channels():
        if ch['group'] == 'raw' and ch['dye_index']:
            raw[ch['dye_index']] = [int(v) for v in ch['values']]
    if dyes is not None:
        raw = {k: v for k, v in raw.items() if k <= dyes}
    return [raw[k] for k in sorted(raw)], a


def make_matrix(standards, dyes):
    """MatrixMaker: ``standards`` = one dict per dye (in dye order) with ``traces`` (list of raw
    filter traces), ``start`` and ``points``; returns the n x n matrix (row = filter, column = dye)."""
    if dyes not in (4, 5):
        raise MatrixError('dyes', 'Number of dyes must be 4 or 5')
    if len(standards) != dyes:
        raise MatrixError('samples', 'Please make sure you have chosen required number of samples')
    windows = []
    for s in standards:
        traces, name = s['traces'], s.get('filename', '')
        if len(traces) != dyes:
            raise MatrixError('dimension', 'The number of dyes (filters) does not match the matrix dimension', name)
        start = int(s['start'])
        length = len(traces[0])
        end = min(start + int(s['points']) - 1, length - 1)
        if start < 0 or end - start + 1 < MIN_POINTS:
            raise MatrixError('points', f'{name} does not have enough data points over the selected range', name)
        windows.append((start, end))
    m = [[0.0] * dyes for _ in range(dyes)]
    for i, s in enumerate(standards):
        start, end = windows[i]
        n = end - start + 1
        data = [genescan_baseline(smooth7(tr[start:start + n])) for tr in s['traces']]
        ref = data[i]
        for j in range(dyes):
            if j == i:
                m[j][i] = 1.0
                continue
            xs, ys = [], []
            t = 0
            while len(xs) < MAX_PAIRS and t < len(data[j]):
                if ref[t] >= PEAK_RFU:
                    xs.append(float(ref[t]))
                    ys.append(float(data[j][t]))
                t += 1
            if len(xs) < MIN_PAIRS:
                raise MatrixError('peaks', f"{s.get('filename', '')} does not have enough peaks in the selected range",
                                  s.get('filename'))
            s1 = medfit(xs, ys)[1]
            xs2 = [0.0] + xs[:-1]
            ys2 = [0.0] + ys[:-1]
            s2 = linear_fit(xs2, ys2)[1]
            v = s1 if s1 >= s2 else s2
            m[j][i] = v if v >= 0.0 else 0.0
    return [[round4(v) for v in row] for row in m]


def invert(m):
    n = len(m)
    a = [list(map(float, row)) + [1.0 if i == j else 0.0 for j in range(n)] for i, row in enumerate(m)]
    for c in range(n):
        p = max(range(c, n), key=lambda r: abs(a[r][c]))
        if abs(a[p][c]) < 1e-12:
            raise MatrixError('singular', 'The matrix cannot be inverted')
        a[c], a[p] = a[p], a[c]
        pv = a[c][c]
        a[c] = [v / pv for v in a[c]]
        for r in range(n):
            if r != c and a[r][c]:
                f = a[r][c]
                a[r] = [a[r][k] - f * a[c][k] for k in range(2 * n)]
    return [row[n:] for row in a]


def validate_matrix(m):
    if not isinstance(m, list) or len(m) not in (4, 5) or any(not isinstance(r, list) or len(r) != len(m) for r in m):
        raise MatrixError('shape', 'Matrix must be 4 x 4 or 5 x 5')
    out = []
    for r in m:
        row = []
        for v in r:
            if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or v < 0 or v > 2:
                raise MatrixError('values', 'Please enter numeric values between 0 and 2')
            row.append(float(v))
        out.append(row)
    invert(out)
    return out


def multicomponent(traces, inverse, int16=True):
    """MultiComponenter: dye_c[t] = sum_r filter_r[t] * inverse[c][r], truncated (int16 wrap for short data)."""
    n = len(inverse)
    if len(traces) != n:
        raise MatrixError('dimension', 'Matrix has wrong dimension')
    length = len(traces[0])
    out = []
    for c in range(n):
        w = inverse[c]
        row = []
        for t in range(length):
            v = _d2i(sum(traces[r][t] * w[r] for r in range(n)))
            if int16:
                v = ((v + 32768) & 0xFFFF) - 32768
            row.append(v)
        out.append(row)
    return out


# ---------- ABIF .mtx files ----------
def _entry(name, number, etype, esize, count, payload):
    return (name.encode('ascii'), number, etype, esize, count, payload)


def write_abif(entries):
    """Minimal ABIF v1.01 writer: header + data + directory (inline payloads of <= 4 bytes)."""
    header_len = 128
    blobs = bytearray()
    dir_entries = []
    for name, number, etype, esize, count, payload in entries:
        size = len(payload)
        if size <= 4:
            offset_field = payload.ljust(4, b'\0')
            dir_entries.append(struct.pack('>4siHHII', name, number, etype, esize, count, size) + offset_field + b'\0\0\0\0')
        else:
            off = header_len + len(blobs)
            blobs += payload
            dir_entries.append(struct.pack('>4siHHIIII', name, number, etype, esize, count, size, off, 0))
    dir_off = header_len + len(blobs)
    root = struct.pack('>4siHHIIII', b'tdir', 1, 1023, 28, len(dir_entries), 28 * len(dir_entries), dir_off, 0)
    head = b'ABIF' + struct.pack('>H', 101) + root
    head = head.ljust(header_len, b'\xff')
    return bytes(head + blobs + b''.join(dir_entries))


def matrix_to_mtx(m, comment=''):
    n = len(m)
    inv = invert(m)
    entries = []
    if comment:
        c = comment.encode('ascii', 'replace')[:255]
        entries.append(_entry('CMNT', 1, 18, 1, len(c) + 1, bytes([len(c)]) + c))
    raw = [float(round4(v)) for row in m for v in row]
    entries.append(_entry('MRAW', 1, 7, 4, len(raw), struct.pack(f'>{len(raw)}f', *raw)))
    entries.append(_entry('MTRD', 1, 4, 2, 1, struct.pack('>h', n)))
    mtrx = [((_d2i(v * 1000.0) + 32768) & 0xFFFF) - 32768 for row in inv for v in row]
    entries.append(_entry('MTRX', 1, 4, 2, len(mtrx), struct.pack(f'>{len(mtrx)}h', *mtrx)))
    return write_abif(entries)


def mtx_to_matrix(data, filename='matrix.mtx'):
    try:
        a = AbifFile(data, filename)
    except AbifError as e:
        raise MatrixError('file', str(e)) from None
    dim = a.value('MTRD')
    if not isinstance(dim, int) or dim not in (4, 5):
        raise MatrixError('dimension', 'Error reading the dimension')
    raw = a.value('MRAW')
    comment = a.text('CMNT')
    if isinstance(raw, list):
        if len(raw) != dim * dim:
            raise MatrixError('values', 'The dimension info and the raw value info in the file is not consistent')
        m = [[round4(raw[r * dim + c]) for c in range(dim)] for r in range(dim)]
        return {'dyes': dim, 'matrix': m, 'comment': comment, 'source': 'MRAW'}
    mtrx = a.value('MTRX')
    if not isinstance(mtrx, list) or len(mtrx) != dim * dim:
        raise MatrixError('values', 'No matrix values are found in the file')
    inv = [[mtrx[r * dim + c] / 1000.0 for c in range(dim)] for r in range(dim)]
    m = [[round4(v) for v in row] for row in invert(inv)]
    return {'dyes': dim, 'matrix': m, 'comment': comment, 'source': 'MTRX'}


def is_310(abif):
    text = ' '.join(str(abif.value(t) or '') for t in ('MODL', 'MCHN', 'HCFG'))
    return '310' in text and '3100' not in text and '3130' not in text


def embedded_matrix(abif):
    """A 310 sample file may carry its own matrix (MTRD/MTRX) — then no library matrix is needed."""
    dim = abif.value('MTRD')
    mtrx = abif.value('MTRX')
    if isinstance(dim, int) and dim in (4, 5) and isinstance(mtrx, list) and len(mtrx) == dim * dim:
        return [[mtrx[r * dim + c] / 1000.0 for c in range(dim)] for r in range(dim)]
    return None


def _b64(data, label, limit=64 * 1024 * 1024):
    import base64
    if not isinstance(data, str):
        raise MatrixError('file', f'{label}: file data required')
    raw = base64.b64decode(data, validate=True)
    if len(raw) > limit:
        raise MatrixError('file', f'{label}: file too large')
    return raw


def matrix_request(request):
    """/api/matrix: make (Matrix Editor Create), export/import .mtx, validate, sniff (310 + embedded matrix)."""
    import base64
    if not isinstance(request, dict):
        raise ValueError('Matrix request must be an object')
    action = request.get('action')
    try:
        if action == 'make':
            dyes = request.get('dyes')
            items = request.get('standards')
            if type(dyes) is not int or not isinstance(items, list):
                raise MatrixError('samples', 'Please make sure you have chosen required number of samples')
            standards = []
            for k in range(dyes):
                it = items[k] if k < len(items) else None
                letter = DYE_LETTERS[k] if k < len(DYE_LETTERS) else str(k + 1)
                if not isinstance(it, dict) or not it.get('fsa_b64'):
                    raise MatrixError('no_file', f'No File Selected for "{letter}" Data')
                name = str(it.get('filename') or f'{letter}.fsa')
                traces, _ = raw_traces(_b64(it['fsa_b64'], name), name, dyes)
                start, points = it.get('start', 1000), it.get('points', 100000)
                if type(start) is not int or type(points) is not int or start < 0 or points < 1:
                    raise MatrixError('points', f'{name}: Start At and Points must be whole numbers')
                standards.append({'traces': traces, 'start': start, 'points': points, 'filename': name})
            m = make_matrix(standards, dyes)
            return {'matrix': m, 'inverse': invert(m)}
        if action == 'validate':
            m = validate_matrix(request.get('matrix'))
            return {'matrix': m, 'inverse': invert(m)}
        if action == 'export':
            m = validate_matrix(request.get('matrix'))
            return {'mtx_b64': base64.b64encode(matrix_to_mtx(m, str(request.get('comment') or ''))).decode('ascii')}
        if action == 'import':
            return mtx_to_matrix(_b64(request.get('mtx_b64'), 'matrix', 1024 * 1024), str(request.get('filename') or 'matrix.mtx'))
        if action == 'sniff':
            a = AbifFile(_b64(request.get('fsa_b64'), 'sample'), str(request.get('filename') or 'sample.fsa'))
            return {'is_310': is_310(a), 'embedded': embedded_matrix(a) is not None, 'model': a.text('MODL')}
    except MatrixError as e:
        return {'error': str(e), 'code': e.code, 'filename': e.filename}
    raise ValueError('action must be make, validate, export, import or sniff')
