"""Profile Manager import/export and the Profile Comparison tool (ID-X 1.6).

* % Match = reference profile alleles found in the comparison profile /
  total reference profile alleles x 100 (help: "percent match").
* Sample concordance groups samples with 100 % concordant profiles (all
  markers, all alleles); a group is mixed-source when any marker has more than
  two alleles, otherwise single-source.
* Comparisons use groups as one unit plus ungrouped samples; for each pair the
  direction with the higher % Match is reported, and only results at or above
  the Percent Match Threshold (50..100, default 80) are listed.
* Allele labels listed as drop-out indicators (GeneMapper_Defaults
  ALLELE_DROP_OUT_INDICATOR, up to 5) are ignored on both sides.
"""
import csv
import io
import re

MAX_PROFILES = 5000
MAX_MARKERS = 128
MAX_ALLELES = 8
PROFILE_TYPES = ('Lab Reference', 'Custom Control')


def _profile(raw, where='profile'):
    if not isinstance(raw, dict):
        raise ValueError(f'{where}: object required')
    pid = str(raw.get('id', '')).strip()
    if not pid or len(pid) > 120:
        raise ValueError(f'{where}: id of 1..120 characters required')
    markers = raw.get('markers')
    if not isinstance(markers, dict) or len(markers) > MAX_MARKERS:
        raise ValueError(f'{pid}: markers object (at most {MAX_MARKERS}) required')
    out = {}
    for name, alleles in markers.items():
        if not isinstance(alleles, list) or len(alleles) > 64:
            raise ValueError(f'{pid}/{name}: allele list required')
        labels = [str(a).strip() for a in alleles if str(a).strip()]
        out[str(name)] = labels
    return {'id': pid, 'markers': out, **({'type': raw['type']} if raw.get('type') in PROFILE_TYPES else {})}


def _clean(profile, ignore):
    return {m: [a for a in alleles if a not in ignore] for m, alleles in profile['markers'].items()}


def percent_match(reference, comparison, ignore=()):
    """Reference alleles found in the comparison profile / total reference alleles x 100."""
    ignore = set(ignore)
    ref, comp = _clean(reference, ignore), _clean(comparison, ignore)
    total = found = 0
    per_marker = {}
    for marker, alleles in ref.items():
        other = set(comp.get(marker, []))
        hits = [a for a in alleles if a in other]
        total += len(alleles)
        found += len(hits)
        per_marker[marker] = {'reference': alleles, 'comparison': comp.get(marker, []), 'matched': hits}
    return (100.0 * found / total if total else 0.0), found, total, per_marker


def _key(profile):
    return tuple(sorted((m, tuple(sorted(a))) for m, a in profile['markers'].items() if a))


def concordance_groups(samples):
    """Group samples with identical (100 % concordant) profiles; groups have two or more members."""
    buckets = {}
    for s in samples:
        k = _key(s)
        if k:
            buckets.setdefault(k, []).append(s)
    groups = []
    for members in buckets.values():
        if len(members) < 2:
            continue
        mixed = any(len(a) > 2 for a in members[0]['markers'].values())
        groups.append({'id': ('Mixed' if mixed else 'Single') + f'-source group {len(groups) + 1}',
                       'kind': 'mixed' if mixed else 'single', 'members': [m['id'] for m in members],
                       'markers': members[0]['markers']})
    return groups


def _units(samples, groups):
    grouped = {mid for g in groups for mid in g['members']}
    units = [{'id': g['id'], 'markers': g['markers'], 'members': g['members'], 'group': g['kind']} for g in groups]
    units += [{'id': s['id'], 'markers': s['markers'], 'members': [s['id']], 'group': None}
              for s in samples if s['id'] not in grouped]
    return units


def _best(a, b, ignore):
    ab = percent_match(a, b, ignore)
    ba = percent_match(b, a, ignore)
    if ba[0] > ab[0]:
        return b, a, ba
    return a, b, ab


def _row(ref, comp, result):
    pct, found, total, per_marker = result
    return {'reference': ref['id'], 'reference_members': ref.get('members', [ref['id']]),
            'comparison': comp['id'], 'comparison_members': comp.get('members', [comp['id']]),
            'percent': round(pct, 1), 'found': found, 'total': total, 'markers': per_marker}


def _threshold(value):
    t = 80 if value is None else float(value)
    if not 50 <= t <= 100:
        raise ValueError('Percent Match Threshold must be 50..100')
    return t


def compare(request):
    """mode: concordance | samples | lab_reference | control."""
    if not isinstance(request, dict):
        raise ValueError('Profile comparison request must be an object')
    mode = request.get('mode', 'concordance')
    samples = request.get('samples') or []
    if not isinstance(samples, list) or len(samples) > MAX_PROFILES:
        raise ValueError(f'At most {MAX_PROFILES} samples')
    samples = [_profile(s, 'sample') for s in samples]
    ignore = [str(x) for x in (request.get('ignore') or [])][:5]
    groups = concordance_groups(samples)
    if mode == 'concordance':
        return {'mode': mode, 'groups': groups}
    threshold = _threshold(request.get('threshold'))
    rows = []
    if mode == 'samples':
        units = _units(samples, groups)
        for i, a in enumerate(units):
            for b in units[i + 1:]:
                ref, comp, result = _best(a, b, ignore)
                if result[0] >= threshold:
                    rows.append(_row(ref, comp, result))
    elif mode in ('lab_reference', 'control'):
        profiles = [_profile(p, 'profile') for p in request.get('profiles') or []]
        if mode == 'lab_reference':
            chosen = set(request.get('selected') or [s['id'] for s in samples])
            targets = [{'id': s['id'], 'markers': s['markers'], 'members': [s['id']]} for s in samples if s['id'] in chosen]
        else:
            targets = _units(samples, groups)
        for p in profiles:
            for t in targets:
                ref, comp, result = _best(p, t, ignore)
                if result[0] >= threshold:
                    rows.append(_row(ref, comp, result))
    else:
        raise ValueError('mode must be concordance, samples, lab_reference or control')
    rows.sort(key=lambda r: (-r['percent'], r['reference'], r['comparison']))
    return {'mode': mode, 'threshold': threshold, 'groups': groups, 'rows': rows}


def export_csv(rows):
    """CSV of comparison results; discordant reference alleles are shown in < >."""
    out = io.StringIO()
    w = csv.writer(out, lineterminator='\r\n')
    w.writerow(['Reference', 'Comparison', '% Match', 'Comment', 'Marker', 'Reference Alleles', 'Comparison Alleles'])
    for r in rows:
        for marker, m in r['markers'].items():
            ref = ', '.join(a if a in m['matched'] else f'<{a}>' for a in m['reference'])
            w.writerow([r['reference'], r['comparison'], r['percent'], r.get('comment', ''), marker, ref, ', '.join(m['comparison'])])
    return out.getvalue()


def parse_profiles(text):
    """Profile import file (tab or comma delimited) with Profile ID, UD7 (type), Marker and Allele 1..8 columns."""
    if not isinstance(text, str) or len(text) > 16 * 1024 * 1024:
        raise ValueError('Profile file text required (at most 16 MiB)')
    first = text.split('\n', 1)[0]
    delim = '\t' if first.count('\t') >= first.count(',') else ','
    reader = csv.reader(io.StringIO(text), delimiter=delim)
    header = [h.strip() for h in next(reader, [])]
    need = ('Profile ID', 'Marker')
    for col in need:
        if col not in header:
            raise ValueError(f'Profile import file needs a {col!r} column')
    idx = {h: i for i, h in enumerate(header)}
    allele_cols = [idx[h] for h in header if re.fullmatch(r'Allele \d+', h)][:MAX_ALLELES]
    type_col = idx.get('UD7', idx.get('Profile Type'))
    profiles = {}
    for row in reader:
        if not row or not any(c.strip() for c in row):
            continue
        get = lambda i: row[i].strip() if i is not None and i < len(row) else ''
        pid, marker = get(idx['Profile ID']), get(idx['Marker'])
        if not pid or not marker:
            continue
        kind = get(type_col) if type_col is not None else ''
        kind = 'Custom Control' if kind.lower().startswith('custom') else 'Lab Reference'
        p = profiles.setdefault(pid, {'id': pid, 'type': kind, 'markers': {}, 'dye': {}, 'size': {}})
        alleles = [get(i) for i in allele_cols if get(i)]
        p['markers'].setdefault(marker, [])
        p['markers'][marker] += [a for a in alleles if a not in p['markers'][marker]]
        if 'Dye' in idx and get(idx['Dye']):
            p['dye'][marker] = get(idx['Dye'])
        if 'Size 1' in idx and get(idx['Size 1']):
            try:
                p['size'][marker] = float(get(idx['Size 1']))
            except ValueError:
                pass
        if len(profiles) > MAX_PROFILES:
            raise ValueError(f'At most {MAX_PROFILES} profiles')
    return list(profiles.values())


def export_profiles(profiles):
    """All profiles in one tab-delimited file (Profile Manager export: no dye or marker size)."""
    rows = [_profile(p) for p in profiles]
    width = max([1] + [len(a) for p in rows for a in p['markers'].values()])
    width = min(max(width, 2), MAX_ALLELES)
    lines = ['\t'.join(['Profile ID', 'UD7', 'Marker'] + [f'Allele {i + 1}' for i in range(width)])]
    for raw, p in zip(profiles, rows):
        for marker, alleles in p['markers'].items():
            cells = alleles[:width] + [''] * (width - len(alleles[:width]))
            lines.append('\t'.join([p['id'], raw.get('type', 'Lab Reference'), marker] + cells))
    return '\r\n'.join(lines) + '\r\n'


def profile_request(request):
    action = request.get('action') if isinstance(request, dict) else None
    if action == 'compare':
        return compare(request)
    if action == 'export_csv':
        return {'text': export_csv(request.get('rows') or [])}
    if action == 'parse':
        return {'profiles': parse_profiles(request.get('text'))}
    if action == 'export':
        return {'text': export_profiles(request.get('profiles') or [])}
    raise ValueError('action must be compare, export_csv, parse or export')
