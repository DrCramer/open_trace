"""Mixture Analysis (GeneMapper ID-X 1.6 Mixture Analysis tool).

Sources: ID-X help "Mixture Analysis" chapters and the decompiled mamodule
(SamplePreProcessor, AlleleProcessor, AnalysisMethodPage) for the factory
method and the segregation rules.

* Method: Mixture Interpretation Threshold (MIT) and heterozygote peak height
  ratio thresholds per peak height range (factory 50-150: 0.40, 151-300: 0.54,
  301-1000: 0.63, 1001-99999: 0.75). Threshold lookup follows
  ``AlleleProcessor.a(List, double)``: first row for heights from MIT-1 up to
  its maximum, middle rows by range, otherwise the last row.
* Segregation (``SamplePreProcessor``): alleles below the MIT are removed
  (the marker gets an ADBI check). More than four alleles at any marker -> 3 or
  more contributors; a four-allele marker or more than one three-allele marker ->
  2 contributors; otherwise a two-allele marker whose ratio is not above its PHR
  threshold -> 2 contributors; else 1 contributor.
* Extraction of 2-contributor samples: genotype combinations (C1 major, C2 minor)
  per marker, the marker Mx is the lowest minor share (4-peak markers; 3-peak
  markers whose heterozygote passes PHR; 3-peak markers with a dropout F only
  when no other candidates exist), the average Mx is their mean. Residual =
  sum of squared differences between expected proportions (Mx/2 per minor allele
  copy, (1-Mx)/2 per major allele copy) and observed proportions; threshold 0.04.
  An F allele has height MIT-1. PHR1/PHR2, PHR status and IQ follow the help
  tables; combinations with IQ Pass are selected automatically.
* Statistics: genotype frequency p^2 + p(1-p)theta (homozygote), 2pq
  (heterozygote), 2p (possible dropout); combined frequency per marker = sum of
  the selected genotypes (an F genotype absorbs genotypes with the same allele);
  profile frequency = product; RMP = 1 / profile frequency; marker inclusion
  frequency = (sum p)^2 (1.0 with a dropout); CPI = product; CPE = 1 - CPI.
  Unobserved alleles or frequencies below 5/2N use the minimum frequency 5/2N.
  Amelogenin and Y markers are excluded from statistics.
"""
from itertools import combinations
import re

RESIDUAL_THRESHOLD = 0.04
FACTORY_METHOD = {'name': 'Default', 'description': 'Factory settings (demonstration values, validate before use)',
                  'mit': 50, 'phr': [[50, 150, 0.4], [151, 300, 0.54], [301, 1000, 0.63], [1001, 99999, 0.75]]}
F = 'F'
is_f = lambda a: a == 'F' or (len(a) == 2 and a[0] == 'F' and a[1] in '123')


# ---------- method ----------
def validate_method(method):
    if not isinstance(method, dict):
        raise ValueError('Mixture analysis method must be an object')
    name = str(method.get('name', '')).strip()
    if not name or len(name) > 120 or re.search(r'[\\/:*?"<>|]', name):
        raise ValueError('Mixture analysis method name: 1..120 characters without \\ / : * ? " < > |')
    mit = method.get('mit')
    if not isinstance(mit, (int, float)) or int(mit) != mit or not 1 < mit < 10000:
        raise ValueError('The MIT value must be a whole number between 1 and 10,000 (exclusive)')
    rows = method.get('phr')
    if not isinstance(rows, list) or not rows or len(rows) > 50:
        raise ValueError('At least one heterozygote PHR setting is required')
    prev = None
    out = []
    for row in rows:
        if not isinstance(row, (list, tuple)) or len(row) != 3:
            raise ValueError('PHR setting rows are [min, max, ratio]')
        lo, hi, ratio = int(row[0]), int(row[1]), float(row[2])
        if not (0 <= lo <= hi <= 99999) or not (0 < ratio <= 1):
            raise ValueError('PHR setting: 0 <= min <= max <= 99999 and 0 < ratio <= 1')
        if prev is not None and lo != prev + 1:
            raise ValueError('Peak height ranges must occur in sequence')
        prev = hi
        out.append([lo, hi, ratio])
    return {'name': name, 'description': str(method.get('description', ''))[:500], 'mit': int(mit), 'phr': out}


def phr_threshold(phr, height, mit):
    """``AlleleProcessor.a(List, double)``."""
    if not phr:
        return 0.0
    if mit - 1 <= height <= phr[0][1]:
        return phr[0][2]
    for lo, hi, ratio in phr[1:-1]:
        if lo <= height <= hi:
            return ratio
    return phr[-1][2]


# ---------- samples ----------
def _sample(raw):
    if not isinstance(raw, dict) or not isinstance(raw.get('markers'), list):
        raise ValueError('Mixture sample needs id and markers')
    markers = []
    for m in raw['markers']:
        alleles = [{'label': str(a['label']), 'height': float(a['height'])} for a in m.get('alleles', [])
                   if a.get('label') not in (None, '')]
        markers.append({'name': str(m['name']), 'y': bool(m.get('y')), 'alleles': alleles})
    return {'id': str(raw.get('id', '')), 'markers': markers}


def eligible(raw):
    """OL-only labels make a sample ineligible ("OL<6" is acceptable)."""
    s = _sample(raw)
    if any(a['label'].upper() == 'OL' for m in s['markers'] for a in m['alleles']):
        return False, 'off_ladder'
    if not any(m['alleles'] for m in s['markers']):
        return False, 'no_genotypes'
    if all(m['y'] for m in s['markers'] if m['alleles']):
        return False, 'y_panel'
    return True, None


def _above_mit(sample, mit):
    adbi = []
    out = []
    for m in sample['markers']:
        kept = [a for a in m['alleles'] if a['height'] >= mit]
        if len(kept) != len(m['alleles']):
            adbi.append(m['name'])
        out.append({**m, 'alleles': kept})
    return out, adbi


def segregate(raw, method):
    method = validate_method(method)
    sample = _sample(raw)
    markers, adbi = _above_mit(sample, method['mit'])
    counts = [len(m['alleles']) for m in markers]
    most = max(counts) if counts else 0
    three = sum(1 for c in counts if c == 3)
    reason = None
    if most > 4:
        category, reason = 3, 'more_than_four_alleles'
    elif most == 4 or three > 1:
        category, reason = 2, 'four_alleles' if most == 4 else 'several_three_allele_markers'
    else:
        category = 1
        for m in markers:
            if len(m['alleles']) != 2:
                continue
            h1, h2 = m['alleles'][0]['height'], m['alleles'][1]['height']
            lo, hi = min(h1, h2), max(h1, h2)
            if not lo / hi > phr_threshold(method['phr'], hi, method['mit']):
                category, reason = 2, 'phr_fail:' + m['name']
                break
    return {'id': sample['id'], 'contributors': category, 'reason': reason, 'adbi_markers': adbi,
            'max_alleles': most}


# ---------- 2-contributor extraction (mamodule AlleleProcessor / Pattern / AnalysisManager) ----------
# Pattern IDs: digits are positions of the called alleles (in their order); the left pair
# carries the calculated share x and the expected share m (minor, C2), the right pair is
# the major contributor (C1). See research/IDX16_MIXTURE_PATTERNS.md.
def _expected_vector(pid, m):
    p, q = m / 2.0, (1.0 - m) / 2.0
    table = {
        '1112': (p + .5, q, 0, 0), '1122': (m, 1 - m, 0, 0), '1123': (m, q, q, 0), '1211': (1 - p, p, 0, 0),
        '1212': (.5, .5, 0, 0), '1213': (.5, p, q, 0), '1222': (p, 1 - p, 0, 0), '1223': (p, .5, q, 0),
        '1233': (p, p, 1 - m, 0), '1234': (p, p, q, q), '1312': (.5, q, p, 0), '1322': (p, 1 - m, p, 0),
        '1323': (p, q, .5, 0), '1324': (p, q, p, q), '1423': (p, q, q, p), '2211': (1 - m, m, 0, 0),
        '2212': (q, p + .5, 0, 0), '2213': (q, m, q, 0), '2311': (1 - m, p, p, 0), '2312': (q, .5, p, 0),
        '2313': (q, p, .5, 0), '2314': (q, p, p, q), '2413': (q, p, q, p), '3312': (q, q, m, 0),
        '3412': (q, q, p, p)}
    return table.get(pid, (0.0, 0.0, 0.0, 0.0))


class _Gen:
    """``AlleleProcessor.G(n, marker, m)`` for one call (strict=false)."""

    def __init__(self, method, names, heights, positive_only=False):
        self.phr, self.t = method['phr'], method['mit']
        self.names = list(names) + [''] * (4 - len(names))
        self.h = list(heights) + [0.0] * (4 - len(heights))
        self.n = len(names)
        self.positive_only = positive_only
        self.r = sum(1 for x in self.h if x >= self.t)

    def q(self, h):
        return phr_threshold(self.phr, h, self.t)

    def pair(self, x, y):
        hi = max(x, y)
        if hi <= 0:
            return False
        return min(x, y) / hi >= self.q(hi)

    def shared(self, s, u, v):
        lo = (u * self.q(u) if u > 0 else 0.0) + (v * self.q(v) if v > 0 else 0.0)
        hi = (u / self.q(u) if u > 0 and self.q(u) else 0.0) + (v / self.q(v) if v > 0 and self.q(v) else 0.0)
        return lo <= s <= hi

    def observed(self):
        total = sum(self.h)
        return tuple(x / total for x in self.h) if total > 0 else (0.0, 0.0, 0.0, 0.0)

    def pattern(self, pid, x, phr, ratios=(None, None)):
        left = (self.names[int(pid[0]) - 1], self.names[int(pid[1]) - 1])
        right = (self.names[int(pid[2]) - 1], self.names[int(pid[3]) - 1])
        return {'id': pid, 'c2': list(left), 'c1': list(right), 'x': x, 'phr': phr,
                'phr2': ratios[0], 'phr1': ratios[1]}

    def ratio(self, i, j):
        a, b = self.h[i], self.h[j]
        return min(a, b) / max(a, b) if max(a, b) > 0 else None

    def generate(self, amel=False):
        h, out = self.h, []
        if amel:
            if self.n == 2 and h[0] >= self.t and h[1] >= self.t:
                out += [self.pattern('1112', 0.0, '?'), self.pattern('1212', 0.0, 'pass' if self.pair(h[0], h[1]) else 'fail',
                                                                     (self.ratio(0, 1), self.ratio(0, 1))),
                        self.pattern('1211', 0.0, '?')]
            elif self.n == 1:
                self.names[1], self.h[1] = 'Y', self.t - 1.0
                out += [self.pattern('1111', 0.0, '?'), self.pattern('1211', 0.0, '?'), self.pattern('1112', 0.0, '?'),
                        self.pattern('1212', 0.0, 'pass' if self.pair(self.h[0], self.h[1]) else 'fail')]
            return out
        if self.n == 1:
            p = self.pattern('1111', 0.5, '?')
            p['observed'] = (0.0, 0.0, 0.0, 0.0)
            return [p]
        if self.n == 2:
            s = h[0] + h[1]
            v = self.pair(h[0], h[1])
            for pid, x in (('1122', h[0] / s if s else 0.0), ('2211', h[1] / s if s else 0.0)):
                if not self.positive_only or x > 0:
                    out.append(self.pattern(pid, x, '?'))
            out.append(self.pattern('1212', 0.0, 'pass' if v else 'fail', (self.ratio(0, 1), self.ratio(0, 1))))
            out += [self.pattern(pid, 0.0, '?') for pid in ('1112', '2212', '1211', '1222')]
            return out
        if self.n == 3:
            H = h[0] + h[1] + h[2]
            rows = [  # id, x numerator/denominator, validity, gate r>1
                ('2311', h[1] + h[2], H, ('P', 1, 2), False), ('1123', h[0], H, ('P', 1, 2), False),
                ('1322', h[0] + h[2], H, ('P', 0, 2), False), ('2213', h[1], H, ('P', 0, 2), False),
                ('1233', h[0] + h[1], H, ('P', 0, 1), True), ('3312', h[2], H, ('P', 0, 1), True),
                ('1223', h[0], h[0] + h[2], ('S', 1, 0, 2), False), ('2312', h[2], h[0] + h[2], ('S', 1, 0, 2), False),
                ('1213', h[1], h[1] + h[2], ('S', 0, 1, 2), False), ('1312', h[2], h[1] + h[2], ('S', 0, 1, 2), True),
                ('1323', h[0], h[0] + h[1], ('S', 2, 0, 1), True), ('2313', h[1], h[0] + h[1], ('S', 2, 0, 1), True)]
            for pid, num, den, test, gate in rows:
                if gate and not self.r > 1:
                    continue
                if test[0] == 'P':
                    valid = self.pair(h[test[1]], h[test[2]])
                    rat = self.ratio(test[1], test[2])
                    # the heterozygous pair is the left (minor) or the right (major) genotype
                    ratios = (rat, None) if pid[0] != pid[1] else (None, rat)
                else:
                    s, u, v = h[test[1]], h[test[2]], h[test[3]]
                    valid = self.shared(s, u, v)
                    rat = min(s, u + v) / max(s, u + v) if max(s, u + v) > 0 else None
                    ratios = (rat, rat)
                x = num / den if valid and den > 0 else 0.0
                if self.positive_only and x <= 0:
                    continue
                out.append(self.pattern(pid, x, 'pass' if valid else 'fail', ratios))
            return out
        if self.n == 4:
            H = sum(h)
            rows = [('1234', 0, 1, 2, 3, 0), ('3412', 2, 3, 0, 1, 0), ('1324', 0, 2, 1, 3, 1), ('2413', 1, 3, 0, 2, 1),
                    ('1423', 0, 3, 1, 2, 2), ('2314', 1, 2, 0, 3, 2)]
            for pid, a, b, c, d, gate in rows:
                if not self.r > gate:
                    continue
                valid = self.pair(h[a], h[b]) and self.pair(h[c], h[d])
                x = (h[a] + h[b]) / H if H > 0 else 0.0
                if self.positive_only and x <= 0:
                    continue
                out.append(self.pattern(pid, x, 'pass' if valid else 'fail', (self.ratio(a, b), self.ratio(c, d))))
            return out
        return out


def _min_x(patterns):
    """``AlleleProcessor.a(d)``: first strictly smaller value, 0 is the "unset" sentinel."""
    best = 0.0
    for p in patterns:
        if best == 0.0 or p['x'] < best:
            best = p['x']
    return best


def _iq(res_pass, phr):
    if res_pass:
        return 'pass' if phr in ('pass', '?') else 'check'
    return 'low' if phr == 'fail' else 'check'


def _is_amel(name):
    return 'AMEL' in name.upper()


def known_genotype_match(genotype, known):
    """mamodule AnalysisManager.a(String, String): exact or reversed pair; F,F matches anything; one F
    matches when the other allele is one of the known alleles."""
    g = [('F' if is_f(a) else str(a)) for a in genotype]
    k = [str(a) for a in known]
    if len(g) != 2 or len(k) != 2:
        return False
    if sorted(g) == sorted(k):
        return True
    a, b = g
    if a == 'F' and b == 'F':
        return True
    if a == 'F':
        return b in k
    if b == 'F':
        return a in k
    return False


def normalize_known(known):
    """{marker: [alleles]}: homozygote stored as one label -> doubled; empty markers dropped; >2 alleles rejected
    (ID-X: "Known genotypes containing tri-allelic markers cannot be imported")."""
    out = {}
    for marker, alleles in (known or {}).items():
        al = [str(a).strip() for a in (alleles or []) if str(a).strip()]
        if not al:
            continue
        if len(al) > 2:
            raise ValueError(f'Known genotype contains a tri-allelic marker ({marker}); remove it before importing')
        out[str(marker)] = al * 2 if len(al) == 1 else al
    return out


def parse_known_genotypes(text, filename):
    """Known genotype file: a Genotypes table exported for one single-source sample (tab or comma separated,
    columns Sample File / Sample Name / Marker / Allele 1..n)."""
    import csv
    import io
    stem = str(filename or '').rsplit('/', 1)[-1].rsplit('\\', 1)[-1]
    stem = stem.rsplit('.', 1)[0] if '.' in stem else stem
    if not stem or stem.lower() == 'import' or any(c in stem for c in '\\/:*?<>|"'):
        raise ValueError('Invalid known genotype file name')
    lines = text.splitlines()
    if not lines:
        raise ValueError('Empty known genotype file')
    delim = '\t' if lines[0].count('\t') >= lines[0].count(',') else ','
    rows = list(csv.reader(io.StringIO(text), delimiter=delim))
    head = [h.strip() for h in rows[0]]
    low = [h.lower() for h in head]
    if 'marker' not in low:
        raise ValueError('Known genotype file needs a Marker column')
    mi = low.index('marker')
    ai = [i for i, h in enumerate(low) if h.startswith('allele')]
    if not ai:
        raise ValueError('Known genotype file needs Allele columns')
    si = low.index('sample name') if 'sample name' in low else (low.index('sample file') if 'sample file' in low else None)
    samples, markers = set(), {}
    for r in rows[1:]:
        if not any(x.strip() for x in r) or len(r) <= mi:
            continue
        if si is not None and si < len(r) and r[si].strip():
            samples.add(r[si].strip())
        name = r[mi].strip()
        if not name:
            continue
        al = [r[i].strip() for i in ai if i < len(r) and r[i].strip()]
        if name in markers and markers[name] != al:
            raise ValueError(f'Marker {name} appears twice with different alleles')
        markers[name] = al
    if len(samples) > 1:
        raise ValueError('The file contains genotypes of more than one sample; export a single-source sample from the Samples Plot')
    profile = normalize_known(markers)
    if not profile:
        raise ValueError('The known genotype file contains no alleles')
    return {'id': stem, 'markers': profile, 'sample': next(iter(samples), '')}


def merge_known(existing, new):
    """Same name: conflicting markers -> not imported; otherwise markers missing from the existing profile are added."""
    for m, al in new['markers'].items():
        if m in existing['markers'] and sorted(existing['markers'][m]) != sorted(al):
            raise ValueError(f'Known genotype {new["id"]} conflicts with the existing profile at {m}; not imported')
    merged = dict(existing['markers'])
    for m, al in new['markers'].items():
        merged.setdefault(m, al)
    return {**existing, 'markers': merged}


def extract(raw, method, known=None):
    """Two-phase extraction of a 2-contributor sample (mamodule AnalysisManager)."""
    method = validate_method(method)
    sample = _sample(raw)
    markers, adbi = _above_mit(sample, method['mit'])
    t = method['mit']
    work = [m for m in markers if not m['y'] and m['alleles']]
    # Phase I: average Mx from 4- and 3-peak markers (positive shares only); fallback 3F.
    used = []
    for m in work:
        n = len(m['alleles'])
        if n in (3, 4) and not _is_amel(m['name']):
            g = _Gen(method, [a['label'] for a in m['alleles']], [a['height'] for a in m['alleles']], positive_only=True)
            x = _min_x(g.generate())
            if x > 0:
                used.append((m['name'], x, n))
    fallback = False
    if not used:
        for m in work:
            if len(m['alleles']) == 3 and not _is_amel(m['name']):
                g = _Gen(method, [a['label'] for a in m['alleles']] + ['F1'], [a['height'] for a in m['alleles']] + [t - 1.0],
                         positive_only=True)
                x = _min_x(g.generate())
                if x > 0:
                    used.append((m['name'], x, '3F'))
        fallback = bool(used)
    avg = sum(x for _, x, _ in used) / len(used) if used else 0.0
    if avg <= 0:
        return {'id': sample['id'], 'status': 'no_mx', 'average_mx': None, 'mx_markers': [], 'rows': [],
                'residual_threshold': RESIDUAL_THRESHOLD, 'adbi_markers': adbi, 'missing_markers': [],
                'message': 'Average Mx cannot be calculated: no 3- or 4-peak markers'}
    # Phase II: every marker, with synthetic F1/F2/F3 (height MIT-1) up to four alleles.
    rows = []
    for m in work:
        names = [a['label'] for a in m['alleles']][:4]
        heights = [a['height'] for a in m['alleles']][:4]
        calls = [(list(names), list(heights))]
        if not _is_amel(m['name']):
            k = 1
            while len(names) < 4 and t - 1 > 0:
                names, heights = names + [f'F{k}'], heights + [t - 1.0]
                calls.append((list(names), list(heights)))
                k += 1
        for cn, ch in calls:
            g = _Gen(method, cn, ch)
            for p in g.generate(amel=_is_amel(m['name'])):
                obs = p.pop('observed', None) or g.observed()
                if p['id'] == '1111':
                    exp = (0.0, 0.0, 0.0, 0.0)
                else:
                    exp = _expected_vector(p['id'], avg)
                residual = sum((e - o) ** 2 for e, o in zip(exp, obs))
                res_pass = residual <= RESIDUAL_THRESHOLD
                p.update(marker=m['name'], residual=residual, residual_status='pass' if res_pass else 'low',
                         phr_status={'pass': 'pass', 'fail': 'low', '?': 'na'}[p['phr']],
                         iq=_iq(res_pass, p['phr']), adbi='check' if m['name'] in adbi else 'pass',
                         dropout=any(is_f(a) for a in p['c1'] + p['c2']),
                         mx=p.pop('x'), inconclusive={'c1': False, 'c2': False})
                p['selected'] = p['iq'] == 'pass'
                del p['phr']
                rows.append(p)
    known_match = None
    known_contributor = None
    if known:
        known = normalize_known(known)
        # mamodule AnalysisManager: (1) for markers of the known profile select only IQ-pass combinations whose C1 or
        # C2 genotype matches the known genotype; (2) count markers where a selected C1 / C2 genotype matches;
        # (3) both -> C1 (1:1 mixture), C1 / C2 exact, otherwise undetermined; (4) for an exact match keep only the
        # selected combinations whose matched contributor carries the known genotype; markers missing from the
        # known profile keep their IQ-pass combinations.
        for r in rows:
            k = known.get(r['marker'])
            r['known'] = list(k) if k else []
            if k:
                r['known_match'] = ('C1' if known_genotype_match(r['c1'], k) else '') + ('C2' if known_genotype_match(r['c2'], k) else '')
                r['selected'] = r['iq'] == 'pass' and bool(r['known_match'])
            else:
                r['known_match'] = ''
        hits = {'C1': 0, 'C2': 0}
        counted = 0
        for mk in sorted({m for m in known if any(r['marker'] == m for r in rows)}):
            sel = [r for r in rows if r['marker'] == mk and r['selected']]
            any_rows = [r for r in rows if r['marker'] == mk]
            if not any_rows:
                continue
            counted += 1
            if any('C1' in r['known_match'] for r in sel):
                hits['C1'] += 1
            if any('C2' in r['known_match'] for r in sel):
                hits['C2'] += 1
        if counted and hits['C1'] == counted:
            known_contributor = 'c1'
        elif counted and hits['C2'] == counted:
            known_contributor = 'c2'
        if known_contributor:
            key = known_contributor.upper()
            for r in rows:
                if r['selected'] and r['known'] and key not in r['known_match']:
                    r['selected'] = False
            known_match = 'C1 (Major)' if key == 'C1' else 'C2 (Minor)'
        else:
            known_match = 'Undetermined'
            known_contributor = None
            lean = 'C1' if hits['C1'] >= hits['C2'] else 'C2'
            for r in rows:
                r['known_lean'] = lean
    present = {r['marker'] for r in rows}
    missing = sorted(present - {r['marker'] for r in rows if r['selected']})
    return {'id': sample['id'], 'status': 'extracted', 'average_mx': avg, 'fallback_3f': fallback,
            'mx_markers': [{'marker': k, 'mx': v, 'peaks': n} for k, v, n in used],
            'residual_threshold': RESIDUAL_THRESHOLD, 'adbi_markers': adbi, 'rows': rows, 'missing_markers': missing,
            'known_match': known_match, 'known_contributor': known_contributor,
            'known_missing_markers': sorted(present - set(known or {})) if known else []}


# ---------- population databases ----------
def parse_population_db(text):
    """GeneMapper ID-X population database (CSV): Module/Version/Database/Data Source header,
    Population=..,Theta=.. ; Marker=..,N=.. ; Allele,Frequency rows ('*' = minimum frequency)."""
    if not isinstance(text, str) or len(text) > 8 * 1024 * 1024:
        raise ValueError('Population database text required (at most 8 MiB)')
    head = {}
    pops = []
    pop = marker = None
    for raw in text.splitlines():
        line = raw.strip().rstrip(',').strip()
        if not line:
            continue
        cells = [c.strip() for c in line.split(',')]
        key = cells[0].split('=', 1)[0].strip().lower()
        if '=' in cells[0] and key in ('module', 'version', 'database', 'data source'):
            head[key] = line.split('=', 1)[1].strip()
            continue
        if key == 'population':
            vals = dict(c.split('=', 1) for c in cells if '=' in c)
            theta = float(vals.get('Theta', vals.get('theta', '0')) or 0)
            if not 0 <= theta <= 1:
                raise ValueError('Theta must be between 0 and 1')
            pop = {'name': vals.get('Population', '').strip(), 'theta': theta, 'markers': {}}
            pops.append(pop)
            marker = None
            continue
        if key == 'marker':
            if pop is None:
                raise ValueError('Marker line before any Population line')
            vals = dict(c.split('=', 1) for c in cells if '=' in c)
            n = int(vals.get('N', vals.get('n', '0')))
            if not 3 <= n <= 99999:
                raise ValueError(f'{vals.get("Marker")}: N must be 3..99999')
            marker = {'n': n, 'freq': {}}
            pop['markers'][vals['Marker'].strip()] = marker
            continue
        if cells[0].lower() == 'allele':
            continue
        if marker is None or len(cells) < 2:
            raise ValueError(f'Unexpected line in population database: {raw[:60]!r}')
        freq = cells[1]
        if freq == '*':
            marker['freq'][cells[0]] = None
        else:
            v = float(freq)
            if not 0 <= v < 1:
                raise ValueError(f'Allele frequency must be >= 0 and < 1: {cells[0]}={freq}')
            marker['freq'][cells[0]] = v
    if 'module' not in head or 'database' not in head or not pops:
        raise ValueError('Not a GeneMapper ID-X population database (Module, Database and Population lines required)')
    return {'module': head['module'], 'version': head.get('version', '1.0'), 'database': head['database'],
            'source': head.get('data source', ''), 'populations': pops}


def allele_frequency(pop, marker, allele):
    m = pop['markers'].get(marker)
    if m is None:
        return None
    minimum = 5.0 / (2.0 * m['n'])
    v = m['freq'].get(allele)
    return minimum if v is None or v < minimum else v


def _stat_marker(name):
    return name.upper() not in ('AMEL', 'AMELOGENIN') and not name.upper().startswith(('DYS', 'Y'))


def genotype_frequency(pop, marker, a, b):
    a, b = (F if is_f(a) else a), (F if is_f(b) else b)
    if a == F and b == F:
        return 1.0  # marker dropout: uninformative
    if b == F or a == F:
        p = allele_frequency(pop, marker, a if b == F else b)
        return None if p is None else 2 * p
    p = allele_frequency(pop, marker, a)
    if p is None:
        return None
    if a == b:
        return p * p + p * (1 - p) * pop['theta']
    q = allele_frequency(pop, marker, b)
    return None if q is None else 2 * p * q


def combined_frequency(pop, marker, genotypes):
    """Sum of distinct genotype frequencies; an (a, F) genotype absorbs the genotypes containing a."""
    norm = [[F if is_f(x) else x for x in g] for g in genotypes]
    if any(g[0] == F and g[1] == F for g in norm):
        return 1.0
    gts = {tuple(sorted(g, key=lambda x: (x == F, x))) for g in norm}
    drop = {g[0] for g in gts if g[1] == F}
    total = 0.0
    for g in sorted(gts):
        if g[1] != F and (g[0] in drop or g[1] in drop):
            continue
        f = genotype_frequency(pop, marker, *g)
        if f is None:
            return None
        total += f
    return total


def rmp(profile, pop):
    """profile = {marker: {'genotypes': [[a,b],...], 'inconclusive': bool}} -> per-marker and total."""
    details, missing, product = [], [], 1.0
    for marker, item in profile.items():
        if not _stat_marker(marker):
            continue
        if item.get('inconclusive') or not item.get('genotypes'):
            details.append({'marker': marker, 'frequency': 1.0, 'note': 'inconclusive'})
            continue
        f = combined_frequency(pop, marker, item['genotypes'])
        if f is None:
            missing.append(marker)
            continue
        product *= f
        details.append({'marker': marker, 'frequency': f, 'genotypes': item['genotypes']})
    return {'profile_frequency': product, 'rmp': (1.0 / product) if product > 0 else None,
            'missing_markers': missing, 'details': details}


def single_source_profile(markers, options=None):
    """1-contributor genotypes from allele lists with ID-X statistical options:
    one allele: alleles (homozygote) | 2p | inconclusive; two: alleles | inconclusive;
    three: min (minimum heterozygous genotype frequency) | inconclusive."""
    options = options or {}
    prof = {}
    for marker, alleles in markers.items():
        opt = options.get(marker) or ('min' if len(alleles) == 3 else 'alleles')
        if opt == 'inconclusive' or not alleles:
            prof[marker] = {'genotypes': [], 'inconclusive': True}
        elif len(alleles) == 1:
            prof[marker] = {'genotypes': [[alleles[0], F]] if opt == '2p' else [[alleles[0], alleles[0]]]}
        elif len(alleles) == 2:
            prof[marker] = {'genotypes': [sorted(alleles)]}
        else:
            prof[marker] = {'genotypes': [sorted(p) for p in combinations(alleles, 2)], 'min': True}
    return prof


def rmp_single(markers, pop, options=None):
    prof = single_source_profile(markers, options)
    out = {'details': [], 'missing_markers': []}
    product = 1.0
    for marker, item in prof.items():
        if not _stat_marker(marker):
            continue
        if item.get('inconclusive'):
            out['details'].append({'marker': marker, 'frequency': 1.0, 'note': 'inconclusive'})
            continue
        freqs = [genotype_frequency(pop, marker, *g) for g in item['genotypes']]
        if any(f is None for f in freqs):
            out['missing_markers'].append(marker)
            continue
        f = min(freqs) if item.get('min') else freqs[0]
        product *= f
        out['details'].append({'marker': marker, 'frequency': f, 'genotypes': item['genotypes']})
    out['profile_frequency'] = product
    out['rmp'] = 1.0 / product if product > 0 else None
    return out


def cpi(markers, pop, dropout_markers=()):
    """markers = {marker: [alleles above MIT]}; markers with a dropout use inclusion frequency 1.0."""
    details, missing, product = [], [], 1.0
    for marker, alleles in markers.items():
        if not _stat_marker(marker) or not alleles:
            continue
        if marker in dropout_markers:
            details.append({'marker': marker, 'alleles': alleles, 'inclusion': 1.0, 'combinations': None})
            continue
        ps = [allele_frequency(pop, marker, a) for a in alleles]
        if any(p is None for p in ps):
            missing.append(marker)
            continue
        mif = sum(ps) ** 2
        n = len(alleles)
        product *= mif
        details.append({'marker': marker, 'alleles': alleles, 'inclusion': mif, 'combinations': n * (n + 1) // 2})
    return {'cpi': product, 'one_in': (1.0 / product) if product > 0 else None, 'cpe': 1.0 - product,
            'missing_markers': missing, 'details': details}


def statistics(request):
    """{kind: 'single'|'two'|'cpi', population_db, populations:[names], ...}"""
    db = request.get('population_db')
    if isinstance(db, str):
        db = parse_population_db(db)
    if not isinstance(db, dict) or not db.get('populations'):
        raise ValueError('Select a population database')
    names = request.get('populations') or [p['name'] for p in db['populations']]
    pops = [p for p in db['populations'] if p['name'] in names]
    if not pops:
        raise ValueError('Select at least one population')
    kind = request.get('kind')
    out = []
    for pop in pops:
        row = {'population': pop['name'], 'theta': pop['theta']}
        if kind == 'single':
            row.update(rmp_single(request.get('markers') or {}, pop, request.get('options')))
        elif kind == 'two':
            known_c = request.get('known_contributor')
            if known_c not in (None, 'c1', 'c2'):
                raise ValueError('known_contributor must be c1 or c2')
            # ID-X: with a matched known genotype statistics are calculated only for the unknown contributor
            row['c1'] = None if known_c == 'c1' else rmp(request.get('c1') or {}, pop)
            row['c2'] = None if known_c == 'c2' else rmp(request.get('c2') or {}, pop)
            row['known_contributor'] = known_c
        elif kind == 'cpi':
            row.update(cpi(request.get('markers') or {}, pop, set(request.get('dropout_markers') or [])))
        else:
            raise ValueError('kind must be single, two or cpi')
        out.append(row)
    return {'database': db['database'], 'kind': kind, 'results': out}


# ---------- likelihood ratio (ID-X "Selecting a Likelihood Ratio (LR) Hypothesis") ----------
def _norm(g):
    return tuple(sorted((F if is_f(a) else a) for a in g))


def _ref_fits(ref, gt):
    """A reference genotype fits an evidence genotype; F (possible dropout) accepts any missing allele."""
    ref = list(ref) * 2 if len(ref) == 1 else list(ref)  # profiles store a homozygote as one label
    gt, ref = _norm(gt), tuple(sorted(ref))
    if F not in gt:
        return gt == ref
    known = [a for a in gt if a != F]
    rest = list(ref)
    for a in known:
        if a not in rest:
            return False
        rest.remove(a)
    return True


HYPOTHESES = {
    'single': ('C1: R1', 'C1: unknown'),
    'major': ('C1: R1', 'C1: unknown'),
    'h1': ('R1 + R2', 'R1 + unknown'),
    'h2': ('R1 + R2', 'R2 + unknown'),
    'h3': ('R1 + R2', 'two unknowns'),
    'h4': ('R1 + unknown', 'two unknowns'),
    'h5': ('R2 + unknown', 'two unknowns'),
}


def _positions(rows, ref, markers):
    """Contributor position(s) at which a reference fits a selected combination at every marker."""
    out = []
    for key in ('c1', 'c2'):
        if all(any(_ref_fits(ref[m], r[key]) for r in rows if r['marker'] == m) for m in markers):
            out.append(key)
    return out


def likelihood_ratio(request):
    """LR = Pr(E|Hp) / Pr(E|Hd) per marker product.

    kind 'single': evidence {marker: alleles} with 1-contributor options; reference R1.
    kind 'two': evidence rows = selected genotype combinations {marker, c1, c2, inconclusive};
    references R1 (and R2); use_c1_only treats the major contributor as single source.
    Pr(E|H) at a marker = sum over selected combinations consistent with the known contributors
    of the genotype frequencies of the unknown contributors (1 when all are known).
    """
    db = request.get('population_db')
    if isinstance(db, str):
        db = parse_population_db(db)
    names = request.get('populations') or [p['name'] for p in db['populations']]
    pops = [p for p in db['populations'] if p['name'] in names]
    if not pops:
        raise ValueError('Select at least one population')
    refs = [r for r in (request.get('references') or []) if r]
    if not refs:
        raise ValueError('Select a reference sample')
    refs = [{'id': str(r['id']), 'markers': {m: [str(a) for a in v] for m, v in r['markers'].items() if v}} for r in refs]
    kind = request.get('kind')
    if kind == 'single':
        evidence = request.get('markers') or {}
        options = request.get('options') or {}
        r1 = refs[0]
        markers = [m for m, a in evidence.items() if a and _stat_marker(m) and options.get(m) != 'inconclusive']
        not_in_ref = [m for m in markers if m not in r1['markers']]
        markers = [m for m in markers if m in r1['markers']]
        mismatch = []
        for m in markers:
            ev = evidence[m]
            ref = r1['markers'][m]
            ok = (sorted(set(ref)) == sorted(set(ev))) if len(ev) <= 2 else set(ref) <= set(ev)
            if len(ev) == 1 and options.get(m) == '2p':
                ok = ev[0] in ref
            if not ok:
                mismatch.append(m)
        status = {'R1': 'match' if not mismatch else 'no_match', 'mismatch': mismatch}
        hyps = ['single'] if not mismatch else []
        results = []
        for pop in pops:
            for h in hyps:
                rows, missing, num, den = [], [], 1.0, 1.0
                for m in markers:
                    f = rmp_single({m: evidence[m]}, pop, options)
                    if f['missing_markers']:
                        missing.append(m)
                        continue
                    d = f['details'][0]['frequency']
                    num *= 1.0
                    den *= d
                    rows.append({'marker': m, 'hp': 1.0, 'hd': d})
                results.append({'population': pop['name'], 'theta': pop['theta'], 'hypothesis': h, 'hp': HYPOTHESES[h][0],
                                'hd': HYPOTHESES[h][1], 'numerator': num, 'denominator': den,
                                'lr': num / den if den > 0 else None, 'details': rows, 'missing_markers': missing})
        return {'kind': kind, 'references': status, 'not_in_reference': not_in_ref, 'hypotheses': hyps, 'results': results}
    if kind != 'two':
        raise ValueError('kind must be single or two')
    rows = [r for r in request.get('rows') or [] if r.get('selected', True)]
    use_c1 = bool(request.get('use_c1_only'))
    markers = []
    for r in rows:
        if r['marker'] not in markers and _stat_marker(r['marker']):
            markers.append(r['marker'])
    inconc = {m for m in markers if any(r['marker'] == m and (r.get('inconclusive') or {}).get('c1') for r in rows)}
    inconc2 = {m for m in markers if any(r['marker'] == m and (r.get('inconclusive') or {}).get('c2') for r in rows)}
    not_in_ref = sorted({m for m in markers for ref in refs if m not in ref['markers']})
    usable = [m for m in markers if all(m in ref['markers'] for ref in refs)]
    pos = [_positions(rows, ref['markers'], usable) for ref in refs]
    status = {}
    for i, ref in enumerate(refs):
        miss = {k: [m for m in usable if not any(_ref_fits(ref['markers'][m], r[k]) for r in rows if r['marker'] == m)]
                for k in ('c1', 'c2')}
        status[f'R{i + 1}'] = {'id': ref['id'], 'positions': pos[i], 'match': bool(pos[i]),
                               'mismatch_c1': miss['c1'], 'mismatch_c2': miss['c2']}
    hyps = []
    p1 = pos[0]
    p2 = pos[1] if len(refs) > 1 else []
    if use_c1:
        if any(len({_norm(r['c1']) for r in rows if r['marker'] == m}) > 1 for m in usable):
            raise ValueError('Use C1 (Major) Only needs one C1 genotype per marker in the Selected Genotype Combinations table')
        if 'c1' in p1:
            hyps = ['major']
    else:
        if p1:
            hyps.append('h4')
        if len(refs) > 1 and p2:
            hyps.append('h5')
        if len(refs) > 1 and p1 and p2 and any(a != b for a in p1 for b in p2):
            hyps = ['h1', 'h2', 'h3'] + hyps

    def assign():
        """Positions of R1/R2 (R1 prefers C1)."""
        a1 = 'c1' if 'c1' in p1 else (p1[0] if p1 else None)
        a2 = None
        if p2:
            a2 = next((x for x in p2 if x != a1), None)
        return a1, a2

    a1, a2 = assign()

    def prob(m, known, pop):
        """sum over selected combinations at m with known[pos]=ref of product of unknown genotype frequencies."""
        seen, total = set(), 0.0
        for r in rows:
            if r['marker'] != m:
                continue
            if any(not _ref_fits(ref, r[k]) for k, ref in known.items()):
                continue
            unknown = [k for k in ('c1', 'c2') if k not in known]
            key = tuple(_norm(r[k]) for k in unknown)
            if key in seen:
                continue
            seen.add(key)
            f = 1.0
            for k in unknown:
                g = genotype_frequency(pop, m, *_norm(r[k]))
                if g is None:
                    return None
                f *= g
            total += f
        return total

    results = []
    for pop in pops:
        for h in hyps:
            r1m = lambda m: refs[0]['markers'][m]
            r2m = lambda m: refs[1]['markers'][m]
            num = den = 1.0
            details, missing = [], []
            for m in usable:
                if m in inconc or (m in inconc2 and not use_c1):
                    details.append({'marker': m, 'note': 'inconclusive'})
                    continue
                if h == 'major':
                    hp = 1.0
                    seen, hd = set(), 0.0
                    for r in rows:
                        if r['marker'] == m and _norm(r['c1']) not in seen:
                            seen.add(_norm(r['c1']))
                            g = genotype_frequency(pop, m, *_norm(r['c1']))
                            hd = None if g is None or hd is None else hd + g
                else:
                    k1 = {a1: r1m(m)} if a1 else {}
                    k2 = {a2: r2m(m)} if a2 else {}
                    if h == 'h4':
                        hp, hd = prob(m, {p1[0] if a1 is None else a1: r1m(m)}, pop), prob(m, {}, pop)
                    elif h == 'h5':
                        pos5 = a2 or p2[0]
                        hp, hd = prob(m, {pos5: r2m(m)}, pop), prob(m, {}, pop)
                    else:
                        hp = prob(m, {**k1, **k2}, pop)
                        hd = prob(m, k1, pop) if h == 'h1' else prob(m, k2, pop) if h == 'h2' else prob(m, {}, pop)
                if hp is None or hd is None:
                    missing.append(m)
                    continue
                num *= hp
                den *= hd
                details.append({'marker': m, 'hp': hp, 'hd': hd})
            results.append({'population': pop['name'], 'theta': pop['theta'], 'hypothesis': h, 'hp': HYPOTHESES[h][0],
                            'hd': HYPOTHESES[h][1], 'numerator': num, 'denominator': den,
                            'lr': num / den if den > 0 else None, 'details': details, 'missing_markers': missing})
    return {'kind': kind, 'references': status, 'assigned': {'R1': a1, 'R2': a2}, 'not_in_reference': not_in_ref,
            'hypotheses': hyps, 'results': results}


# ---------- merged results (VirtualSample, AnalysisManager.a(VirtualSample)) ----------
def merge(request):
    """Samples with the same name: per marker the first source with selected results is the
    baseline; other sources with selected results must agree (same genotype pairs) -> '<Multiple>',
    otherwise the marker is in conflict and every source variant is offered."""
    sources = request.get('sources') or []
    if len(sources) < 2:
        raise ValueError('Merging needs two or more samples with the same name')
    kind = request.get('kind', 'two')
    order = []
    for s in sources:
        for m in s['markers']:
            if m['name'] not in order:
                order.append(m['name'])
    out = []
    for name in order:
        variants = []
        for s in sources:
            m = next((x for x in s['markers'] if x['name'] == name), None)
            if not m:
                continue
            if kind == 'two':
                sel = [r for r in m['rows'] if r.get('selected')]
                if sel:
                    key = sorted((_norm(r['c1']), _norm(r['c2'])) for r in sel)
                    variants.append({'source': s['id'], 'rows': sel, 'key': key})
            else:
                if m['alleles']:
                    variants.append({'source': s['id'], 'alleles': m['alleles'], 'key': sorted(m['alleles'])})
        if not variants:
            continue
        base = variants[0]
        others = variants[1:]
        if not others:
            status, source = 'single', base['source']
        elif all(v['key'] == base['key'] for v in others):
            status, source = 'concordant', '<Multiple>'
        else:
            status, source = 'conflict', None
        for v in variants:
            v.pop('key')
        out.append({'marker': name, 'status': status, 'source': source,
                    'chosen': None if status == 'conflict' else 0, 'variants': variants})
    return {'kind': kind, 'markers': out}


def mixture_request(request):
    if not isinstance(request, dict):
        raise ValueError('Mixture request must be an object')
    action = request.get('action')
    if action == 'factory':
        return {'method': dict(FACTORY_METHOD), 'residual_threshold': RESIDUAL_THRESHOLD}
    if action == 'validate':
        return {'method': validate_method(request.get('method'))}
    if action == 'segregate':
        method = request.get('method') or FACTORY_METHOD
        out = []
        for s in request.get('samples') or []:
            ok, why = eligible(s)
            out.append(segregate(s, method) if ok else {'id': s.get('id'), 'contributors': None, 'ineligible': why})
        return {'samples': out}
    if action == 'extract':
        return extract(request.get('sample'), request.get('method') or FACTORY_METHOD, request.get('known'))
    if action == 'parse_known':
        return parse_known_genotypes(request.get('text') or '', request.get('filename') or '')
    if action == 'merge_known':
        return merge_known(request.get('existing') or {}, request.get('new') or {})
    if action == 'parse_popdb':
        return parse_population_db(request.get('text'))
    if action == 'statistics':
        return statistics(request)
    if action == 'lr':
        return likelihood_ratio(request)
    if action == 'merge':
        return merge(request)
    raise ValueError('action must be factory, validate, segregate, extract, parse_popdb, statistics, lr or merge')
