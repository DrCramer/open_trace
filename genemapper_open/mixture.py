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
    if known:
        for r in rows:
            k = sorted(known.get(r['marker'], []))
            r['known'] = k
            r['known_match'] = ('C1' if sorted(r['c1']) == k else '') + ('C2' if sorted(r['c2']) == k else '')
        markers_k = {r['marker'] for r in rows if r['marker'] in known and not _is_amel(r['marker'])}
        hits = {}
        for key in ('C1', 'C2'):
            hits[key] = sum(1 for mk in markers_k
                            if any(r['selected'] and key in r['known_match'] for r in rows if r['marker'] == mk))
        known_match = ('C1 (Major)' if markers_k and hits['C1'] == len(markers_k) else
                       'C2 (Minor)' if markers_k and hits['C2'] == len(markers_k) else 'Undetermined')
    present = {r['marker'] for r in rows}
    missing = sorted(present - {r['marker'] for r in rows if r['selected']})
    return {'id': sample['id'], 'status': 'extracted', 'average_mx': avg, 'fallback_3f': fallback,
            'mx_markers': [{'marker': k, 'mx': v, 'peaks': n} for k, v, n in used],
            'residual_threshold': RESIDUAL_THRESHOLD, 'adbi_markers': adbi, 'rows': rows, 'missing_markers': missing,
            'known_match': known_match}


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
            row['c1'] = rmp(request.get('c1') or {}, pop)
            row['c2'] = rmp(request.get('c2') or {}, pop)
        elif kind == 'cpi':
            row.update(cpi(request.get('markers') or {}, pop, set(request.get('dropout_markers') or [])))
        else:
            raise ValueError('kind must be single, two or cpi')
        out.append(row)
    return {'database': db['database'], 'kind': kind, 'results': out}


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
    if action == 'parse_popdb':
        return parse_population_db(request.get('text'))
    if action == 'statistics':
        return statistics(request)
    raise ValueError('action must be factory, validate, segregate, extract, parse_popdb or statistics')
