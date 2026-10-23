"""Read GeneMapper kit text files (Panels, Bins, Marker Stutter) as OpenTrace kits.

Interoperability reader for files that the user supplies; no manufacturer
definitions are bundled with OpenTrace. The tab-separated layout below was
observed in the files shipped with GeneMapper ID-X 1.6 (``Version GMID-X v 1.x``
panels and bins, ``SSFv1.2`` marker stutter).

Panels: ``Chemistry Kit``/``Panel`` header rows, then one row per marker:
name, dye, min bp, max bp, control alleles, repeat, option, Y-marker flag,
ladder alleles (comma separated, trailing comma).
Bins: ``Panel Name``/``Marker Name`` rows, then label, center, left, right and
an optional ``virtual`` flag. Stutter: ``Panel Name``/``Marker Name`` rows, then
ratio, from bp, to bp and ``Minus``/``Plus``.

Analysis-method values (cut-off, Minus A, global stutter, ladder cut-off) are not
part of these files; they come from the explicit ``method`` argument, whose
defaults are OpenTrace choices, not the original factory defaults.
"""
from copy import deepcopy
import math
import re

from .kit_definition import load_kit_definition

MAX_TEXT_BYTES = 2 * 1024 * 1024
DEFAULT_DYE_CHANNELS = {'blue': 'DATA1', 'green': 'DATA2', 'yellow': 'DATA3', 'red': 'DATA4',
                        'orange': 'DATA105', 'purple': 'DATA106'}
DEFAULT_METHOD = {
    'use_marker_specific_stutter': True,
    'cutoff_ratio': 0.0,
    'ladder_cutoff_ratio': 0.0,
    'minus_a_ratio': 0.0, 'minus_a_min_bp': 0.75, 'minus_a_max_bp': 1.25,
    'global_minus_stutter': {'ratio': 0.0, 'min_bp': 3.25, 'max_bp': 4.75},
    'global_plus_stutter': {'ratio': 0.0, 'min_bp': 3.25, 'max_bp': 4.75},
}


class KitFormatError(ValueError):
    """Malformed or unsupported GeneMapper kit text."""


def _lines(text, what):
    if isinstance(text, bytes):
        if len(text) > MAX_TEXT_BYTES:
            raise KitFormatError(f'{what} file exceeds 2 MiB')
        for encoding in ('utf-8-sig', 'cp1252'):
            try:
                text = text.decode(encoding)
                break
            except UnicodeDecodeError:
                continue
    if not isinstance(text, str):
        raise KitFormatError(f'{what} text required')
    if len(text.encode('utf-8')) > MAX_TEXT_BYTES:
        raise KitFormatError(f'{what} file exceeds 2 MiB')
    rows = []
    for number, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        rows.append((number, [field.strip() for field in line.split('\t')]))
    if not rows:
        raise KitFormatError(f'{what} file is empty')
    return rows


def _float(value, where):
    try:
        number = float(value)
    except ValueError:
        raise KitFormatError(f'{where}: number expected, got {value!r}') from None
    if not math.isfinite(number):
        raise KitFormatError(f'{where}: finite number expected')
    return number


def _labels(value):
    if value.lower() in ('', 'none', 'null'):
        return []
    return [part.strip() for part in value.split(',') if part.strip()]


def parse_panels(text):
    rows = _lines(text, 'Panels')
    header = {}
    kits, kit, panel = [], None, None
    for number, f in rows:
        key = f[0]
        if key == 'Version':
            header['version'] = f[1] if len(f) > 1 else ''
        elif key == 'Kit type:':
            header['kit_type'] = f[1] if len(f) > 1 else ''
            if header['kit_type'] != 'MICROSATELLITE':
                raise KitFormatError(f'line {number}: only MICROSATELLITE kits are supported')
        elif key == 'Chemistry Kit':
            kit = {'name': f[1], 'panels': []}; kits.append(kit); panel = None
        elif key == 'Panel':
            if kit is None:
                raise KitFormatError(f'line {number}: Panel before Chemistry Kit')
            panel = {'name': f[1], 'markers': []}; kit['panels'].append(panel)
        else:
            if panel is None:
                raise KitFormatError(f'line {number}: marker row before Panel')
            if len(f) > 8 and f[1] == '' and f[2]:
                f = [f[0]] + f[2:]  # tolerate a stray empty column after the marker name
            if len(f) < 8:
                raise KitFormatError(f'line {number}: marker row needs at least 8 fields')
            where = f'line {number} ({f[0]})'
            repeat = f[5]
            if not re.fullmatch(r'[0-9]{1,2}', repeat):
                raise KitFormatError(f'{where}: integer repeat expected')
            # Versions differ: no flags (8 fields), Y marker (9), Y marker and
            # internal QC (10). Boolean fields follow the option column in order.
            flags, rest = [], f[7:]
            while rest and rest[0].lower() in ('true', 'false') and len(flags) < 2:
                flags.append(rest.pop(0).lower() == 'true')
            if len(rest) > 1:
                raise KitFormatError(f'{where}: unexpected fields after the marker flags')
            marker = {'name': f[0], 'dye': f[1].lower(), 'min_bp': _float(f[2], where), 'max_bp': _float(f[3], where),
                'control_alleles': _labels(f[4]), 'repeat': int(repeat), 'option': f[6],
                'y_marker': flags[0] if flags else False, 'ladder_alleles': _labels(rest[0]) if rest else []}
            if len(flags) > 1:
                marker['internal_qc'] = flags[1]
            panel['markers'].append(marker)
    if 'version' not in header or not kits:
        raise KitFormatError('Panels file needs Version and Chemistry Kit rows')
    return {'format': 'genemapper_panels', **header, 'kits': kits}


def _blocks(text, what, value_row):
    rows = _lines(text, what)
    header, panels, panel, marker = {}, {}, None, None
    for number, f in rows:
        key = f[0]
        if key == 'Version':
            header['version'] = f[1] if len(f) > 1 else ''
        elif key == 'Chemistry Kit':
            header['kit'] = f[1] if len(f) > 1 else ''
        elif key == 'BinSet Name':
            header['bin_set'] = f[1] if len(f) > 1 else ''
        elif key == 'Panel Name':
            panel = panels.setdefault(f[1], {}); marker = None
        elif key == 'Marker Name':
            if panel is None:
                raise KitFormatError(f'line {number}: Marker Name before Panel Name')
            marker = panel.setdefault(f[1], [])
        else:
            if marker is None:
                raise KitFormatError(f'line {number}: value row before Marker Name')
            marker.append(value_row(number, f))
    if 'version' not in header or not panels:
        raise KitFormatError(f'{what} file needs Version and Panel Name rows')
    return header, panels


def parse_bins(text):
    def row(number, f):
        if len(f) < 4:
            raise KitFormatError(f'line {number}: bin row needs label, center, left, right')
        where = f'line {number} ({f[0]})'
        flag = f[4].lower() if len(f) > 4 else ''
        if flag not in ('', 'virtual'):
            raise KitFormatError(f'{where}: unknown bin flag {f[4]!r}')
        return {'label': f[0], 'center_bp': _float(f[1], where), 'left_bp': _float(f[2], where),
                'right_bp': _float(f[3], where), 'virtual': flag == 'virtual'}
    header, panels = _blocks(text, 'Bins', row)
    return {'format': 'genemapper_bins', **header, 'panels': panels}


def parse_stutter(text):
    def row(number, f):
        if len(f) < 4:
            raise KitFormatError(f'line {number}: stutter row needs ratio, from, to, direction')
        where = f'line {number}'
        direction = f[3].strip().lower()
        if direction not in ('minus', 'plus'):
            raise KitFormatError(f'{where}: direction must be Minus or Plus')
        ratio, low, high = (_float(v, where) for v in f[:3])
        if not 0 <= ratio <= 1 or low > high:
            raise KitFormatError(f'{where}: ratio in [0,1] and from <= to required')
        return {'ratio': ratio, 'min_bp': low, 'max_bp': high, 'direction': direction}
    header, panels = _blocks(text, 'Stutter', row)
    return {'format': 'genemapper_stutter', **header, 'panels': panels}


def list_panels(panels_doc):
    return [{'kit': kit['name'], 'panel': panel['name'], 'markers': len(panel['markers'])}
            for kit in panels_doc['kits'] for panel in kit['panels']]


def _method(method):
    result = deepcopy(DEFAULT_METHOD)
    for key, value in (method or {}).items():
        if key not in result:
            raise ValueError(f'Unknown method field: {key}')
        result[key] = value if not isinstance(value, dict) else {**result[key], **value}
    return result


def build_kit(panels_doc, bins_doc, stutter_doc=None, *, panel=None, method=None, dye_channels=None,
              analysis_method=None):
    """Return a validated OpenTrace kit for one panel of the supplied files.

    ``analysis_method`` (an OpenTrace HID method) supplies the Allele tab values
    per marker repeat and dye; it cannot be combined with ``method``.
    """
    if analysis_method is not None and method is not None:
        raise ValueError('Give either method or analysis_method, not both')
    if analysis_method is not None:
        from .analysis_method import load_analysis_method, kit_marker_parameters
        analysis_method = load_analysis_method(analysis_method)
    choices = [(kit, p) for kit in panels_doc['kits'] for p in kit['panels']]
    if panel is None:
        if len(choices) != 1:
            raise ValueError('Several panels are present; choose one: ' + ', '.join(p['name'] for _, p in choices))
        kit, chosen = choices[0]
    else:
        found = [(k, p) for k, p in choices if p['name'] == panel]
        if not found:
            raise ValueError(f'Panel {panel!r} is not in the Panels file')
        kit, chosen = found[0]
    bins = bins_doc['panels'].get(chosen['name'])
    if bins is None:
        raise ValueError(f'Bins file has no bin set for panel {chosen["name"]!r}')
    stutter = (stutter_doc or {'panels': {}})['panels'].get(chosen['name'], {})
    m = _method(method)
    channels = {**DEFAULT_DYE_CHANNELS, **(dye_channels or {})}
    markers, notes = [], []
    for row in chosen['markers']:
        name = row['name']
        if row['dye'] not in channels:
            raise ValueError(f'{name}: no channel for dye {row["dye"]!r}')
        marker_bins = bins.get(name)
        if not marker_bins:
            raise ValueError(f'{name}: no bins in the bin set')
        low, high = row['min_bp'], row['max_bp']
        outside = [b['label'] for b in marker_bins if not low <= b['center_bp'] <= high]
        if outside:
            notes.append({'code': 'bins_outside_marker_range_excluded', 'marker': name, 'labels': outside})
        kit_bins = [{'label': b['label'], 'center_bp': b['center_bp'], 'left_bp': b['left_bp'],
                     'right_bp': b['right_bp'], 'virtual': b['virtual']}
                    for b in marker_bins if low <= b['center_bp'] <= high]
        known = {b['label'].casefold() for b in kit_bins}
        ladder = [label for label in row['ladder_alleles'] if label.casefold() in known]
        missing = [label for label in row['ladder_alleles'] if label.casefold() not in known]
        if missing:
            notes.append({'code': 'ladder_alleles_without_bins', 'marker': name, 'labels': missing})
        if analysis_method is not None:
            m = {**kit_marker_parameters(analysis_method, row['repeat'], row['dye']),
                 'use_marker_specific_stutter': analysis_method['allele']['use_marker_specific_stutter']}
        specific = stutter.get(name, []) if m['use_marker_specific_stutter'] else []
        minus = [{'min_bp': s['min_bp'], 'max_bp': s['max_bp'], 'ratio': s['ratio']} for s in specific if s['direction'] == 'minus']
        plus = [{'min_bp': s['min_bp'], 'max_bp': s['max_bp'], 'ratio': s['ratio']} for s in specific if s['direction'] == 'plus']
        if not specific:
            # Global settings apply to markers without marker-specific stutter.
            g = m['global_minus_stutter']
            minus = [{'min_bp': g['min_bp'], 'max_bp': g['max_bp'], 'ratio': g['ratio']}]
            g = m['global_plus_stutter']
            plus = [{'min_bp': g['min_bp'], 'max_bp': g['max_bp'], 'ratio': g['ratio']}]
        markers.append({'name': name, 'channel': channels[row['dye']], 'dye': row['dye'],
            'min_bp': low, 'max_bp': high, 'repeat': row['repeat'], 'bins': kit_bins, 'ladder_labels': ladder,
            'control_alleles': row['control_alleles'], 'y_marker': row['y_marker'],
            **({'internal_qc': row['internal_qc']} if 'internal_qc' in row else {}),
            'parameters': {'ladder_cutoff_ratio': m['ladder_cutoff_ratio'], 'cutoff_ratio': m['cutoff_ratio'],
                'minus_a_ratio': m['minus_a_ratio'], 'minus_a_min_bp': m['minus_a_min_bp'],
                'minus_a_max_bp': m['minus_a_max_bp'],
                'backward_stutter_ranges': [r for r in minus if r['ratio'] > 0],
                'forward_stutter_ranges': [r for r in plus if r['ratio'] > 0],
                **({'clear_ranges': m['clear_ranges']} if m.get('clear_ranges') else {})}})
    kit_def = load_kit_definition({'schema_version': 1, 'name': chosen['name'], 'markers': markers})
    kit_def['diagnostics'] = kit_def['diagnostics'] + notes + [{
        'code': 'imported_from_genemapper_text', 'chemistry_kit': kit['name'], 'panel': chosen['name'],
        'bin_set': bins_doc.get('bin_set'), 'panels_version': panels_doc.get('version'),
        'bins_version': bins_doc.get('version'),
        'stutter_version': (stutter_doc or {}).get('version'), 'dye_channels': channels,
        'method': m if analysis_method is None else None,
        'analysis_method': analysis_method['name'] if analysis_method is not None else None}]
    return kit_def


def import_kit(panels_text, bins_text, stutter_text=None, *, panel=None, method=None, dye_channels=None,
               analysis_method=None):
    panels_doc = parse_panels(panels_text)
    bins_doc = parse_bins(bins_text)
    stutter_doc = parse_stutter(stutter_text) if stutter_text else None
    return build_kit(panels_doc, bins_doc, stutter_doc, panel=panel, method=method, dye_channels=dye_channels,
                     analysis_method=analysis_method)


def default_ladder_quality_definition(kit):
    """Explicit OpenTrace ladder QC recipe for an imported kit.

    Height ratio .15 and the bin-spacing curve ``0:0.25`` with a 49 bp limit are
    the compiled defaults of the original preferences (IDX16_PREFERENCES §5-6;
    no GeneMapper_Analysis.cfg override ships). Spike/offscale weights are the
    method's ladder GQ weighting switches, enabled here. Every marker gets the
    same recipe.
    """
    recipe = {'min_height_ratio': 0.15, 'spike_metric': 0, 'spike_weight': 0.5, 'offscale_metric': 0,
              'offscale_weight': 0.5, 'spacing': {'enabled': True, 'max_distance_bp': 49,
              'curve': [{'distance_bp': 0, 'fraction': 0.25, 'interpolate': False}],
              'exceptions': [], 'excluded_alleles': []}}
    return {'schema_version': 1, 'calling_parameters_source': 'ladder_matching_calls',
            'ladder_sq': {'source': 'base_quality'}, 'sq_thresholds': {'good': 0.75, 'bad': 0.25},
            'gq_thresholds': {'good': 0.75, 'bad': 0.25},
            'markers': {m['name']: deepcopy(recipe) for m in kit['markers'] if len(m['ladder_labels']) >= 1}}


def import_request(request):
    """HTTP/CLI helper: parse supplied texts; build a kit when a panel is chosen."""
    if not isinstance(request, dict):
        raise ValueError('Kit import request must be an object')
    texts = {key: request.get(key) for key in ('panels', 'bins', 'stutter')}
    out = {'panels': None, 'bins': None, 'stutter': None, 'panel_list': [], 'kit': None,
           'ladder_quality_definition': None, 'marker_quality_definition': None}
    if texts['panels']:
        out['panels'] = parse_panels(texts['panels'])
        out['panel_list'] = list_panels(out['panels'])
    if texts['bins']:
        out['bins'] = parse_bins(texts['bins'])
    if texts['stutter']:
        out['stutter'] = parse_stutter(texts['stutter'])
    if request.get('panel') is not None:
        if not out['panels'] or not out['bins']:
            raise ValueError('Panels and Bins files are both required to build a kit')
        am = request.get('analysis_method')
        out['kit'] = build_kit(out['panels'], out['bins'], out['stutter'], panel=request['panel'],
                               method=request.get('method'), dye_channels=request.get('dye_channels'),
                               analysis_method=am)
        if am is not None:
            from .analysis_method import ladder_quality_definition, marker_quality_definition, load_analysis_method
            am = load_analysis_method(am)
            out['ladder_quality_definition'] = ladder_quality_definition(am, out['kit'])
            out['marker_quality_definition'] = marker_quality_definition(am, out['kit'])
        else:
            out['ladder_quality_definition'] = default_ladder_quality_definition(out['kit'])
    return out
