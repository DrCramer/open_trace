"""Sample Info, Raw Data and EPT Data views (ID-X View menu) read straight from the sample file.

Item lists follow the ID-X help topic "Displaying Sample Information" (Run Information, Data Collection
Settings, buffers, capillary information); the ABIF tags are those written by 31xx/3500 Data Collection.
EPT arrays are DATA5..8 (voltage, current, power, temperature) in the instrument's own units: 31xx files
store voltage in 10 V steps, 3500 files in kV, so the view reports raw values together with the tag.
"""
from __future__ import annotations

import datetime as _dt

from .abif import AbifFile

RAW_TAGS = (1, 2, 3, 4, 105, 106)
EPT_TAGS = (('voltage', 5), ('current', 6), ('power', 7), ('temperature', 8))


def _date(a, n):
    d, t = a.value('RUND', n), a.value('RUNT', n)
    if not (isinstance(d, (list, tuple)) and len(d) >= 3):
        return None
    try:
        if isinstance(t, (list, tuple)) and len(t) >= 3:
            return _dt.datetime(d[0], d[1], d[2], t[0], t[1], t[2])
        return _dt.datetime(d[0], d[1], d[2])
    except ValueError:
        return None


def _text(a, name, n=1):
    v = a.value(name, n)
    if isinstance(v, str):
        return v.strip()
    if isinstance(v, (int, float)):
        return v
    return None


def _items(pairs):
    return [{'key': k, 'value': v} for k, v in pairs if v not in (None, '')]


def sample_info(data: bytes, filename='sample.fsa'):
    a = AbifFile(data, filename)
    start, stop = _date(a, 1), _date(a, 2)
    duration = None
    if start and stop and stop >= start:
        sec = int((stop - start).total_seconds())
        duration = '%d:%02d:%02d' % (sec // 3600, sec % 3600 // 60, sec % 60)
    raw = []
    for ch in a.channels():
        if ch['group'] == 'raw':
            raw.append({'tag': ch['tag'], 'dye_index': ch['dye_index'], 'dye': ch['dye'], 'values': ch['values']})
    raw.sort(key=lambda c: c['dye_index'])
    ept = {}
    for key, n in EPT_TAGS:
        v = a.value('DATA', n)
        if isinstance(v, list) and v:
            ept[key] = {'tag': f'DATA{n}', 'values': v, 'min': min(v), 'max': max(v)}
    scan = a.value('SCAN') if isinstance(a.value('SCAN'), int) else a.value('Scan')
    sections = [
        {'id': 'sample', 'items': _items([
            ('sampleFile', a.filename), ('sampleName', _text(a, 'SpNm') or _text(a, 'SMPL')), ('well', _text(a, 'TUBE')),
            ('plate', _text(a, 'CTNM')), ('fileSource', f'ABIF v{a.version / 100:.2f}'), ('comment', _text(a, 'CMNT')),
            ('sizeStandardName', _text(a, 'StdF')), ('normalizationFactor', _text(a, 'NrmS')), ('injection', _text(a, 'InjN'))])},
        {'id': 'run', 'items': _items([
            ('userName', _text(a, 'User')), ('instrumentID', _text(a, 'MCHN')),
            ('instrumentType', _text(a, 'HCFG', 3) or _text(a, 'MODL')), ('instrumentConfig', _text(a, 'HCFG', 4)),
            ('dataCollectionVersion', _text(a, 'SVER')), ('firmwareVersion', _text(a, 'SVER', 3)),
            ('runName', _text(a, 'RunN')), ('runDate', start.strftime('%Y-%m-%d %H:%M:%S') if start else None),
            ('runStop', stop.strftime('%Y-%m-%d %H:%M:%S') if stop else None), ('runDuration', duration),
            ('totalDataPoints', scan)])},
        {'id': 'collection', 'items': _items([
            ('moduleFile', _text(a, 'MODF')), ('runModuleName', _text(a, 'RMdN')), ('runModuleVersion', _text(a, 'RMdV')),
            ('runProtocol', _text(a, 'RPrN')), ('runVoltage', _text(a, 'EPVt')), ('injectionVoltage', _text(a, 'InVt')),
            ('injectionDuration', _text(a, 'InSc')), ('temperature', _text(a, 'Tmpr')), ('laserPower', _text(a, 'LsrP')),
            ('dyeSetName', _text(a, 'DySN')), ('dyes', ', '.join(f"{c['dye']}" for c in raw if c['dye']) or None),
            ('polymerType', _text(a, 'GTyp')), ('polymerLot', _text(a, 'SMLt')), ('polymerExpiration', _text(a, 'SMED')),
            ('polymerInstalled', _text(a, 'SMID')), ('resultsGroup', _text(a, 'RGNm'))])},
        {'id': 'buffers', 'items': _items([
            ('anodeType', _text(a, 'ABTp')), ('anodeLot', _text(a, 'ABLt')), ('anodeExpiration', _text(a, 'ABED')),
            ('anodeInstalled', _text(a, 'ABID')), ('cathodeType', _text(a, 'CBTp')), ('cathodeLot', _text(a, 'CBLt')),
            ('cathodeExpiration', _text(a, 'CBED')), ('cathodeInstalled', _text(a, 'CBID'))])},
        {'id': 'capillary', 'items': _items([
            ('arrayLot', _text(a, 'CALt')), ('arraySerial', _text(a, 'CASN')), ('capillaryLength', _text(a, 'LNTD')),
            ('capillaries', _text(a, 'NLNE')), ('capillaryNumber', _text(a, 'LANE')), ('arrayUsage', _text(a, 'CARn')),
            ('arrayExpiration', _text(a, 'CAED'))])},
    ]
    return {'filename': a.filename, 'sections': [s for s in sections if s['items']], 'raw': raw, 'ept': ept,
            'scan_count': scan if isinstance(scan, int) else (len(raw[0]['values']) if raw else 0)}
