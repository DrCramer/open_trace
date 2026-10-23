"""HID analysis method: one object with the tabs of the original editor.

The schema mirrors the parameters of the GeneMapper ID-X Analysis Method Editor
(General, Allele, Peak Detector, Peak Quality, SQ & GQ Settings). Factory values
are taken from ``AnalysisMethod.factoryDefaults()`` (application mode 2, HID) of
ID-X 1.6 and the editor help; see research/IDX16_OPENTRACE23.md. Smoothing
None/Light/Heavy is stored as the original pass counts 0/2/8.

The method is translated into explicit OpenTrace inputs: analyze() detector
settings, kit marker parameters, marker/ladder quality definitions and optional
spike/pull-up definitions. Parameters that OpenTrace cannot honour are reported
in ``unsupported`` instead of being silently ignored.

GeneMapper XML (``AnalysisMethodContainer``) import/export reads and writes the
HID subset only; other elements of an imported file are preserved verbatim on
export of the same method.
"""
from copy import deepcopy
import math
import re
import xml.etree.ElementTree as ET

DYES = ('blue', 'green', 'yellow', 'red', 'purple', 'orange')
REPEATS = {'3': 'tri', '4': 'tetra', '5': 'penta', '6': 'hexa'}
REPEAT_FIELDS = ('cutoff', 'minus_a_ratio', 'minus_a_from', 'minus_a_to', 'minus_stutter_ratio', 'minus_stutter_from',
                 'minus_stutter_to', 'plus_stutter_ratio', 'plus_stutter_from', 'plus_stutter_to')
SMOOTHING = {'none': 0, 'light': 2, 'heavy': 8}
# Stored names as written by the original editor (ParamView constants).
SIZE_CALLING = {'local_southern': 'Local Southern', 'lsq2': 'Least Square Second Order',
                'lsq3': 'Least Square Third Order', 'cubic_spline': 'Cubic Spline Interpolation',
                'global_southern': 'Global Southern'}
CALIBRATION_MODEL = {'local_southern': 'local_rational', 'lsq2': 'lsq2', 'lsq3': 'lsq3', 'cubic_spline': 'natural_cubic'}
WEIGHTS = ('bd', 'bin', 'ovl', 'spk', 'acc', 'cc', 'an', 'lph', 'mph', 'os', 'phr')
DEFAULT_DYE_CHANNELS = {'blue': 'DATA1', 'green': 'DATA2', 'yellow': 'DATA3', 'red': 'DATA4',
                        'orange': 'DATA105', 'purple': 'DATA106'}
NAME_RE = re.compile(r'^[^\x00-\x1f<>&"\\/]{1,240}$')
MAX_XML_BYTES = 1024 * 1024


def _repeat(**values):
    row = dict.fromkeys(REPEAT_FIELDS, 0.0)
    row.update(values)
    return row


FACTORY = {
    'schema_version': 1, 'name': 'OpenTrace HID', 'description': '', 'instrument': '',
    'allele': {
        'bin_set': None, 'use_marker_specific_stutter': True,
        'repeats': {'3': _repeat(), '4': _repeat(minus_stutter_from=3.25, minus_stutter_to=4.75),
                    '5': _repeat(), '6': _repeat()},
        'amelogenin_cutoff': 0.0,
        'range_filter': {dye: {'from': 0.0, 'to': 0.0} for dye in DYES},
    },
    'peak_detector': {
        'analysis_range': {'full': True, 'start': 0, 'stop': 10000},
        'sizing_range': {'all': True, 'start_bp': 0.0, 'stop_bp': 1000.0},
        'smoothing': 'light', 'baseline_window': 51, 'size_calling': 'local_southern',
        'thresholds': dict.fromkeys(DYES, 50), 'min_half_width': 2, 'polynomial_degree': 3,
        'peak_window': 15, 'slope_start': 0.0, 'slope_end': 0.0, 'normalization': False,
    },
    'peak_quality': {
        'lph_homozygous': 200.0, 'lph_heterozygous': 100.0, 'mph': 5000.0, 'phr': 0.7, 'bd_bp': 1.5,
        'max_alleles': 2, 'max_alleles_y': 1, 'ladder_spike': {'enabled': True, 'cutoff': 0.2},
        'sample_spike': True,
    },
    'sq_gq': {
        'weights': {'bd': 0.8, 'bin': 0.8, 'ovl': 0.8, 'spk': 0.3, 'acc': 0.0, 'cc': 1.0, 'an': 1.0,
                    'lph': 0.3, 'mph': 0.3, 'os': 0.8, 'phr': 0.3},
        'sq_weight': 0.5, 'ladder_spike': 1, 'ladder_offscale': 1,
        'sq': {'pass': 0.75, 'low': 0.25}, 'gq': {'pass': 0.75, 'low': 0.25},
    },
    # Values with no editor control in the original HID method; kept explicit.
    'fixed': {'str_peak_window': 9, 'ladder_cutoff': 0.03, 'bleedthrough_height_ratio': 0.05,
              'bleedthrough_scan_distance': 2, 'profile_pullup_threshold': 0.5, 'spike_max_width_bp': 0.4},
}


def factory_method(name=None):
    method = deepcopy(FACTORY)
    if name is not None:
        method['name'] = name
    return load_analysis_method(method)


def _num(value, where, low=-math.inf, high=math.inf):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError(f'{where}: finite number required')
    if not low <= value <= high:
        raise ValueError(f'{where}: value must be in [{low}, {high}]')
    return float(value)


def _int(value, where, low, high):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or value != int(value):
        raise ValueError(f'{where}: integer required')
    if not low <= value <= high:
        raise ValueError(f'{where}: value must be in [{low}, {high}]')
    return int(value)


def _bool(value, where):
    if type(value) is not bool:
        raise ValueError(f'{where}: true or false required')
    return value


def _keys(value, keys, where):
    if not isinstance(value, dict) or set(value) != set(keys):
        missing = set(keys) - set(value) if isinstance(value, dict) else set(keys)
        extra = set(value) - set(keys) if isinstance(value, dict) else set()
        raise ValueError(f'{where}: fields {sorted(keys)} required (missing {sorted(missing)}, unknown {sorted(extra)})')


def load_analysis_method(source):
    """Validate a method mapping and return a normalized copy."""
    m = deepcopy(source)
    _keys(m, FACTORY, 'method')
    if m['schema_version'] != 1 or type(m['schema_version']) is not int:
        raise ValueError('method.schema_version must be 1')
    if not isinstance(m['name'], str) or not NAME_RE.match(m['name'].strip()) or m['name'] != m['name'].strip():
        raise ValueError('method.name: 1..240 characters without control or markup characters')
    for key in ('description', 'instrument'):
        if not isinstance(m[key], str) or len(m[key]) > 2000:
            raise ValueError(f'method.{key}: text up to 2000 characters required')
    a = m['allele']
    _keys(a, FACTORY['allele'], 'allele')
    if a['bin_set'] is not None and (not isinstance(a['bin_set'], str) or not 1 <= len(a['bin_set']) <= 255):
        raise ValueError('allele.bin_set: name or null required')
    _bool(a['use_marker_specific_stutter'], 'allele.use_marker_specific_stutter')
    _keys(a['repeats'], REPEATS, 'allele.repeats')
    for repeat, row in a['repeats'].items():
        where = f'allele.repeats.{REPEATS[repeat]}'
        _keys(row, REPEAT_FIELDS, where)
        for key in REPEAT_FIELDS:
            row[key] = _num(row[key], f'{where}.{key}', 0, 1 if key.endswith(('ratio', 'cutoff')) else 100)
        for kind in ('minus_a', 'minus_stutter', 'plus_stutter'):
            if row[kind+'_from'] > row[kind+'_to']:
                raise ValueError(f'{where}: {kind} From must not exceed To')
    a['amelogenin_cutoff'] = _num(a['amelogenin_cutoff'], 'allele.amelogenin_cutoff', 0, 1)
    _keys(a['range_filter'], DYES, 'allele.range_filter')
    for dye, rng in a['range_filter'].items():
        _keys(rng, ('from', 'to'), f'allele.range_filter.{dye}')
        rng['from'] = _num(rng['from'], f'allele.range_filter.{dye}.from', 0, 100000)
        rng['to'] = _num(rng['to'], f'allele.range_filter.{dye}.to', 0, 100000)
        if rng['from'] > rng['to']:
            raise ValueError(f'allele.range_filter.{dye}: From must not exceed To')
    p = m['peak_detector']
    _keys(p, FACTORY['peak_detector'], 'peak_detector')
    _keys(p['analysis_range'], ('full', 'start', 'stop'), 'peak_detector.analysis_range')
    _bool(p['analysis_range']['full'], 'analysis_range.full')
    for key in ('start', 'stop'):
        p['analysis_range'][key] = _int(p['analysis_range'][key], 'analysis_range.'+key, 0, 10_000_000)
    if p['analysis_range']['stop'] <= p['analysis_range']['start']:
        raise ValueError("analysis_range: 'Stop Point' must be greater than 'Start Point'")
    _keys(p['sizing_range'], ('all', 'start_bp', 'stop_bp'), 'peak_detector.sizing_range')
    _bool(p['sizing_range']['all'], 'sizing_range.all')
    for key in ('start_bp', 'stop_bp'):
        p['sizing_range'][key] = _num(p['sizing_range'][key], 'sizing_range.'+key, 0, 100000)
    if p['sizing_range']['stop_bp'] <= p['sizing_range']['start_bp']:
        raise ValueError("sizing_range: 'Stop Size' must be greater than 'Start Size'")
    if p['smoothing'] not in SMOOTHING:
        raise ValueError('peak_detector.smoothing: none, light or heavy required')
    p['baseline_window'] = _int(p['baseline_window'], 'peak_detector.baseline_window', 3, 4001)
    if p['baseline_window'] % 2 == 0:
        raise ValueError('peak_detector.baseline_window must be odd')
    if p['size_calling'] not in SIZE_CALLING:
        raise ValueError('peak_detector.size_calling: unknown method')
    _keys(p['thresholds'], DYES, 'peak_detector.thresholds')
    for dye in DYES:
        p['thresholds'][dye] = _int(p['thresholds'][dye], 'threshold '+dye, 1, 1_000_000)
    p['min_half_width'] = _int(p['min_half_width'], 'peak_detector.min_half_width', 2, 99)
    p['polynomial_degree'] = _int(p['polynomial_degree'], 'peak_detector.polynomial_degree', 2, 99)
    p['peak_window'] = _int(p['peak_window'], 'peak_detector.peak_window', 5, 101)
    if p['peak_window'] % 2 == 0:
        raise ValueError('peak_detector.peak_window must be an odd number')
    if p['polynomial_degree'] > p['peak_window']:
        raise ValueError('Polynomial Degree cannot be greater than Peak Window Size.')
    p['slope_start'] = _num(p['slope_start'], 'peak_detector.slope_start', 0, 1e12)
    p['slope_end'] = _num(p['slope_end'], 'peak_detector.slope_end', -1e12, 0)
    _bool(p['normalization'], 'peak_detector.normalization')
    q = m['peak_quality']
    _keys(q, FACTORY['peak_quality'], 'peak_quality')
    for key in ('lph_homozygous', 'lph_heterozygous', 'mph'):
        q[key] = _num(q[key], 'peak_quality.'+key, 0, 1e9)
    q['phr'] = _num(q['phr'], 'peak_quality.phr', 0, 1)
    q['bd_bp'] = _num(q['bd_bp'], 'peak_quality.bd_bp', 0, 1000)
    q['max_alleles'] = _int(q['max_alleles'], 'peak_quality.max_alleles', 1, 128)
    q['max_alleles_y'] = _int(q['max_alleles_y'], 'peak_quality.max_alleles_y', 1, 128)
    _keys(q['ladder_spike'], ('enabled', 'cutoff'), 'peak_quality.ladder_spike')
    _bool(q['ladder_spike']['enabled'], 'ladder_spike.enabled')
    q['ladder_spike']['cutoff'] = _num(q['ladder_spike']['cutoff'], 'ladder_spike.cutoff', 0, 1)
    _bool(q['sample_spike'], 'peak_quality.sample_spike')
    s = m['sq_gq']
    _keys(s, FACTORY['sq_gq'], 'sq_gq')
    _keys(s['weights'], WEIGHTS, 'sq_gq.weights')
    for key in WEIGHTS:
        s['weights'][key] = _num(s['weights'][key], 'weight '+key.upper(), 0, 1)
    s['sq_weight'] = _num(s['sq_weight'], 'sq_gq.sq_weight', 0, 1)
    for key in ('ladder_spike', 'ladder_offscale'):
        s[key] = _int(s[key], 'sq_gq.'+key, 0, 1)
    for key in ('sq', 'gq'):
        _keys(s[key], ('pass', 'low'), 'sq_gq.'+key)
        s[key]['pass'] = _num(s[key]['pass'], key+'.pass', 0, 1)
        s[key]['low'] = _num(s[key]['low'], key+'.low', 0, 1)
        if s[key]['low'] >= s[key]['pass']:
            raise ValueError(f'{key.upper()}: Low Quality Range must end below the Pass Range')
    f = m['fixed']
    _keys(f, FACTORY['fixed'], 'fixed')
    f['str_peak_window'] = _int(f['str_peak_window'], 'fixed.str_peak_window', 5, 101)
    if f['str_peak_window'] % 2 == 0:
        raise ValueError('fixed.str_peak_window must be odd')
    f['ladder_cutoff'] = _num(f['ladder_cutoff'], 'fixed.ladder_cutoff', 0, 1)
    f['bleedthrough_height_ratio'] = _num(f['bleedthrough_height_ratio'], 'fixed.bleedthrough_height_ratio', 0, 1)
    f['bleedthrough_scan_distance'] = _int(f['bleedthrough_scan_distance'], 'fixed.bleedthrough_scan_distance', 0, 1000)
    f['profile_pullup_threshold'] = _num(f['profile_pullup_threshold'], 'fixed.profile_pullup_threshold', 0, 1)
    f['spike_max_width_bp'] = _num(f['spike_max_width_bp'], 'fixed.spike_max_width_bp', 0, 100)
    return m


# ---------------------------------------------------------------- engine inputs
def unsupported(method):
    """Settings OpenTrace cannot apply yet; analysis refuses them explicitly."""
    p, out = method['peak_detector'], []
    if p['size_calling'] not in CALIBRATION_MODEL:
        out.append('size_calling_'+p['size_calling'])
    if p['polynomial_degree'] != 3:
        out.append('polynomial_degree_not_3')
    if p['normalization']:
        out.append('normalization')
    if method['allele']['amelogenin_cutoff'] != 0:
        out.append('amelogenin_cutoff')
    return out


def analyze_settings(method, standard_bp, dye_channels=None, standard_dye='orange'):
    """Keyword settings for server.analyze()/ /api/analyze for this method."""
    method = load_analysis_method(method)
    bad = unsupported(method)
    if bad:
        raise ValueError('Analysis method settings not supported by OpenTrace: ' + ', '.join(bad))
    p, f = method['peak_detector'], method['fixed']
    channels = {**DEFAULT_DYE_CHANNELS, **(dye_channels or {})}
    bp = sorted(float(x) for x in standard_bp)
    if not p['sizing_range']['all']:
        bp = [x for x in bp if p['sizing_range']['start_bp'] <= x <= p['sizing_range']['stop_bp']]
    if len(bp) < 4:
        raise ValueError('Sizing range leaves fewer than 4 size-standard fragments')
    settings = {
        'sizing_mode': 'auto', 'analysis_engine': 'morphology_ratio', 'standard_channel': channels[standard_dye],
        'standard_bp': ','.join(f'{x:g}' for x in bp), 'quality_threshold': method['sq_gq']['sq']['pass'],
        'height_mode': 'explicit', 'calibration_model': CALIBRATION_MODEL[p['size_calling']],
        'num_extra_peaks': 25, 'distance': 8, 'max_residual_bp': 5.0,
        'threshold': float(p['thresholds'][standard_dye]),
        'channel_thresholds': {channels[d]: float(p['thresholds'][d]) for d in DYES if d in channels},
        'analysis_range': None if p['analysis_range']['full'] else [p['analysis_range']['start'], p['analysis_range']['stop']],
        'detector': {'baseline_window': p['baseline_window'], 'smooth_passes': SMOOTHING[p['smoothing']],
                     'window': f['str_peak_window'], 'standard_window': p['peak_window'],
                     'min_width': float(p['min_half_width']), 'tau_begin': p['slope_start'], 'tau_end': p['slope_end']},
    }
    if method['peak_quality']['sample_spike']:
        settings['spike_definition'] = spike_definition(method)
    settings['pullup_definition'] = pullup_definition(method, channels, standard_dye)
    return settings


def spike_definition(method):
    """Sample spike detection with the compiled scalar scan thresholds.

    SpikeDetFWHMThresholdScan 3.5 and SpikeDetBaseWidthThresholdScan 5 are the
    compiled scalars; the original replaces them by a curve of the sample's
    mean scans/bp (equal to these values from 8 scans/bp), not applied here.
    """
    ladder = method['peak_quality']['ladder_spike']
    return {'schema_version': 2, 'measurement_source': 'prepared_signal', 'scope': 'non_reference_selected_channels',
            'morphology_enabled': False, 'units': 'scan', 'min_height': 0,
            'exhalf_width_threshold': 3.5, 'near_base_width_threshold': 5, 'area_ratio_threshold': 0.7,
            'treat_overlap_as_spike': True, 'near_base_fraction': 0.06, 'max_base_height': 140,
            # Downstream transitions: spikes at/below the marker cut-off and the
            # Allelic Ladder Spike cut-off (fraction of the highest peak) are cleared.
            'adjustments': {'sample_cutoff': True, 'sample_marker_envelope': True,
                            'ladder_height_ratio': ladder['cutoff'] if ladder['enabled'] else 1e12}}


def pullup_definition(method, channels, standard_dye='orange'):
    f = method['fixed']
    tags = [channels[d] for d in DYES if d in channels]
    standard = channels[standard_dye]
    pairs = [{'candidate_channel': a, 'reference_channel': b} for a in tags if a != standard for b in tags if b != a]
    return {'schema_version': 1, 'profile_threshold': f['profile_pullup_threshold'], 'detector': {
        'algorithm': 'static_ar_pairwise_body', 'calling_policy': 'explicit_ordered_channel_pairs',
        'phase': 'after_spike_restoration_before_calling', 'reference_order': 'stable_scan_midpoint_binary_search',
        'max_scan_distance': f['bleedthrough_scan_distance'], 'height_ratio_threshold': f['bleedthrough_height_ratio'],
        'channel_pairs': pairs[:32]}}


def kit_marker_parameters(method, repeat, dye):
    """Allele tab values for one marker (by repeat and dye) as kit parameters."""
    a = method['allele']
    # Repeats without an editor column (mono/di and non-STR markers such as
    # Amelogenin, repeat 9) use the factory zero values of their own fields.
    row = a['repeats'].get(str(repeat), _repeat())
    rng = a['range_filter'].get(dye, {'from': 0.0, 'to': 0.0})
    return {
        'ladder_cutoff_ratio': method['fixed']['ladder_cutoff'], 'cutoff_ratio': row['cutoff'],
        'minus_a_ratio': row['minus_a_ratio'], 'minus_a_min_bp': row['minus_a_from'], 'minus_a_max_bp': row['minus_a_to'],
        'global_minus_stutter': {'ratio': row['minus_stutter_ratio'], 'min_bp': row['minus_stutter_from'],
                                 'max_bp': row['minus_stutter_to']},
        'global_plus_stutter': {'ratio': row['plus_stutter_ratio'], 'min_bp': row['plus_stutter_from'],
                                'max_bp': row['plus_stutter_to']},
        'clear_ranges': [{'min_bp': rng['from'], 'max_bp': rng['to']}] if rng['to'] > rng['from'] else [],
    }


def marker_quality_definition(method, kit):
    q, w = method['peak_quality'], method['sq_gq']['weights']
    weights = {'allele_number': w['an'], 'absolute_height': w['lph'], 'high_height': w['mph'], 'spike': w['spk'],
               'ratio': w['phr'], 'broad': w['bd'], 'offscale': w['os'], 'overlap': w['ovl'], 'binning': w['bin']}
    markers, amel = {}, None
    for m in kit['markers']:
        if m['name'].upper() in ('AMEL', 'AMELOGENIN'):
            kind, amel = 'amel', m['name']
        else:
            kind = 'y' if m.get('y_marker') else 'autosomal'
        markers[m['name']] = {'kind': kind, 'confidence_source': 'bin_confidence_weight',
            'thresholds': {'max_alleles': q['max_alleles_y'] if kind == 'y' else q['max_alleles'],
                           'homozygous_height': q['lph_homozygous'], 'heterozygous_height': q['lph_heterozygous'],
                           'high_height': q['mph'], 'ratio': q['phr'], 'width_bp': q['bd_bp']},
            'weights': dict(weights), 'flags': dict.fromkeys(('spike', 'offscale', 'overlap', 'binning'), False)}
    return {'schema_version': 1, 'sample_sq': {'source': 'base_quality'},
            'gq_thresholds': {'good': method['sq_gq']['gq']['pass'], 'bad': method['sq_gq']['gq']['low']},
            'amel_marker': amel, 'control_mode': 'none', 'markers': markers}


def ladder_quality_definition(method, kit):
    """Original ladder QC defaults plus the method's ladder spike/off-scale switches."""
    q, s = method['peak_quality'], method['sq_gq']
    recipe = {'min_height_ratio': 0.15,
              'spike_metric': 0, 'spike_weight': 1.0 if (s['ladder_spike'] and q['ladder_spike']['enabled']) else 0.0,
              'offscale_metric': 0, 'offscale_weight': 1.0 if s['ladder_offscale'] else 0.0,
              'spacing': {'enabled': True, 'max_distance_bp': 49,
                          'curve': [{'distance_bp': 0, 'fraction': 0.25, 'interpolate': False}],
                          'exceptions': [], 'excluded_alleles': []}}
    return {'schema_version': 1, 'calling_parameters_source': 'ladder_matching_calls',
            'ladder_sq': {'source': 'base_quality'},
            'sq_thresholds': {'good': s['sq']['pass'], 'bad': s['sq']['low']},
            'gq_thresholds': {'good': s['gq']['pass'], 'bad': s['gq']['low']},
            'markers': {m['name']: deepcopy(recipe) for m in kit['markers'] if m['ladder_labels']}}


# ---------------------------------------------------------------- GeneMapper XML
NS = 'www.AppliedBiosystems.com'
_XML_DYE = {'blue': 'Blue', 'green': 'Green', 'yellow': 'Yellow', 'red': 'Red', 'orange': 'Orange', 'purple': 'Purple'}
_XML_REPEAT = {'3': 'triNucleotide', '4': 'tetriNucleotide', '5': 'pentaNucleotide', '6': 'hexNucleotide'}
_SMOOTH_FROM = {0: 'none', 2: 'light', 8: 'heavy'}


def _local(tag):
    return tag.rsplit('}', 1)[-1]


def _children(node):
    out = {}
    for child in node:
        out.setdefault(_local(child.tag), child)
    return out


def _text(node, name, default=None):
    child = _children(node).get(name) if node is not None else None
    return default if child is None or child.text is None else child.text.strip()


def _f(node, name, default):
    value = _text(node, name)
    try:
        return float(value) if value is not None else float(default)
    except ValueError:
        raise ValueError(f'{name}: number expected, got {value!r}') from None


def _b(node, name, default):
    value = _text(node, name)
    return default if value is None else value.lower() == 'true'


def parse_gm_xml(text):
    """Read an ``AnalysisMethodContainer`` XML (HID subset) as an OpenTrace method."""
    raw = text.encode('utf-8') if isinstance(text, str) else text
    if not isinstance(raw, bytes) or len(raw) > MAX_XML_BYTES:
        raise ValueError('Analysis method XML up to 1 MiB required')
    if b'<!DOCTYPE' in raw or b'<!ENTITY' in raw:
        raise ValueError('DTDs and entities are not accepted in analysis method XML')
    try:
        root = ET.fromstring(raw)
    except ET.ParseError as error:
        raise ValueError(f'Malformed XML: {error}') from None
    if _local(root.tag) != 'AnalysisMethodContainer':
        raise ValueError('AnalysisMethodContainer root element required')
    node = _children(root).get('xmlAnalysisMethod')
    if node is None:
        raise ValueError('xmlAnalysisMethod element required')
    c = _children(node)
    mode = _text(node, 'applicationMode')
    if mode not in (None, '2'):
        raise ValueError(f'Only HID analysis methods (applicationMode 2) are supported, got {mode}')
    m = deepcopy(FACTORY)
    m['name'] = (_text(node, 'analysisMethodName') or 'Imported method').strip()[:240]
    m['description'] = _text(node, 'description', '') or ''
    m['instrument'] = _text(node, 'instrument', '') or ''
    ac, pd, bl, sc = (c.get(k) for k in ('alleleCallerParameter', 'peakDetectorParameter',
                                         'baselinerParameter', 'sizeCallerParameter'))
    a = m['allele']
    if ac is not None:
        a['bin_set'] = _text(ac, 'binSet') or None
        a['use_marker_specific_stutter'] = _b(ac, 'useMarkerStutterPercentage', True)
        for repeat, prefix in _XML_REPEAT.items():
            row = a['repeats'][repeat]
            row['cutoff'] = _f(ac, prefix+'_cutOffValue', 0)
            row['minus_a_ratio'] = _f(ac, prefix+'_minusARatio', 0)
            row['minus_a_from'] = _f(ac, prefix+'_minusADistanceRight', 0)
            row['minus_a_to'] = _f(ac, prefix+'_minusADistance', 0)
            row['minus_stutter_ratio'] = _f(ac, prefix+'_stutterRatio', 0)
            row['minus_stutter_from'] = _f(ac, prefix+'_stutterDistanceRight', 0)
            row['minus_stutter_to'] = _f(ac, prefix+'_stutterDistance', 0)
            row['plus_stutter_ratio'] = _f(ac, prefix+'_plus_stutterRatio', 0)
            row['plus_stutter_from'] = _f(ac, prefix+'_plus_stutterDistanceRight', 0)
            row['plus_stutter_to'] = _f(ac, prefix+'_plus_stutterDistance', 0)
        a['amelogenin_cutoff'] = _f(ac, 'amelogeninCutoff', 0)
        for dye, word in _XML_DYE.items():
            a['range_filter'][dye] = {'from': _f(ac, f'clearRange{word}DyeFrom', 0), 'to': _f(ac, f'clearRange{word}DyeTo', 0)}
        q = m['peak_quality']
        q['phr'] = _f(ac, 'warningPeakHeightRatioThreshold', q['phr'])
        q['lph_homozygous'] = _f(ac, 'warningPeakHeightThresholdHomozygous', q['lph_homozygous'])
        q['lph_heterozygous'] = _f(ac, 'warningPeakHeightThresholdHeterozygous', q['lph_heterozygous'])
        q['mph'] = _f(ac, 'warningHighPeakHeightThreshold', q['mph'])
        q['max_alleles'] = int(_f(ac, 'warningMaxAlleleNumber', q['max_alleles']))
        q['max_alleles_y'] = int(_f(ac, 'warningMaxAlleleNumberForYMarkers', q['max_alleles_y']))
        q['bd_bp'] = _f(ac, 'warningPeakWidthThreshold', q['bd_bp'])
        q['ladder_spike'] = {'enabled': _b(ac, 'spikeDetectionEnabled', True),
                             'cutoff': _f(ac, 'spikeLadderCutoffThreshold', 0.2)}
        q['sample_spike'] = _b(ac, 'spikeDetectionForSampleEnabled', True)
        f = m['fixed']
        f['bleedthrough_height_ratio'] = _f(ac, 'warningBleedthroughHeightThreshold', f['bleedthrough_height_ratio'])
        f['bleedthrough_scan_distance'] = int(_f(ac, 'warningBleedthroughScanThreshold', f['bleedthrough_scan_distance']))
        f['spike_max_width_bp'] = _f(ac, 'spikeMaxPeakWidthThreshold', f['spike_max_width_bp'])
    p = m['peak_detector']
    p['analysis_range'] = {'full': _b(node, 'analysisFullRange', True), 'start': int(_f(node, 'analysisStartPt', 0)),
                           'stop': int(_f(node, 'analysisStopPt', 10000))}
    p['sizing_range'] = {'all': _b(node, 'sizeCallAllSizes', True), 'start_bp': _f(node, 'sizeCallStartSize', 0),
                         'stop_bp': _f(node, 'sizeCallStopSize', 1000)}
    smooth = int(_f(node, 'smoothValue', 2))
    if smooth not in _SMOOTH_FROM:
        raise ValueError(f'smoothValue {smooth} is not None (0), Light (2) or Heavy (8)')
    p['smoothing'] = _SMOOTH_FROM[smooth]
    if bl is not None:
        p['baseline_window'] = int(_f(bl, 'windowSize', 51))
    if sc is not None:
        name = _text(sc, 'sizeCallMethod', 'Local Southern')
        reverse = {v: k for k, v in SIZE_CALLING.items()}
        if name not in reverse:
            raise ValueError(f'Unknown size calling method {name!r}')
        p['size_calling'] = reverse[name]
    if pd is not None:
        p['polynomial_degree'] = int(_f(pd, 'degree', 3))
        p['peak_window'] = int(_f(pd, 'window', 15))
        m['fixed']['str_peak_window'] = int(_f(pd, 'windowTetri', 9))
        p['min_half_width'] = int(_f(pd, 'minWidth', 2))
        p['slope_start'] = _f(pd, 'tauB', 0)
        p['slope_end'] = _f(pd, 'tauE', 0)
        for dye, word in _XML_DYE.items():
            p['thresholds'][dye] = int(_f(pd, 'minHeight'+word, p['thresholds'][dye]))
        p['normalization'] = _b(pd, 'applyNormalization', False)
    qv = c.get('qualityValueProcessParameter')
    s = m['sq_gq']
    if qv is not None:
        mapping = {'bd': 'broadPeakQualityWeight', 'bin': 'binningQualityWeight', 'ovl': 'overlapQualityWeight',
                   'spk': 'spikeMarkerPqvQualityWeight', 'acc': 'amelCCQualityWeight', 'cc': 'controlAlleleQualityWeight',
                   'an': 'alleleNumberMarkerPqvQualityWeight', 'lph': 'peakAbsoluteHeightQualityWeight',
                   'mph': 'highPeakHeightQualityWeight', 'os': 'offscaleQualityWeight', 'phr': 'peakHeightRatioQualityWeight'}
        for key, name in mapping.items():
            s['weights'][key] = _f(qv, name, s['weights'][key])
        s['sq_weight'] = _f(qv, 'BroadPeakSQPqvQualityWeight', s['sq_weight'])
        s['ladder_spike'] = int(_f(qv, 'alSpikeMarkerPqvQualityWeight', 1) >= 0.5)
        s['ladder_offscale'] = int(_f(qv, 'alOffscaleMarkerPqvQualityWeight', 1) >= 0.5)
        m['fixed']['profile_pullup_threshold'] = _f(qv, 'profilePullupQualityThreshold', 0.5)
    for entry in node:
        if _local(entry.tag) != 'thresholdMap':
            continue
        key = _text(entry, 'Key', '')
        qt = _children(entry).get('QualityThreshold')
        target = 'sq' if key.startswith('Sizing') else 'gq' if key.startswith('Genotype') else None
        if target and qt is not None:
            s[target] = {'pass': _f(qt, 'goodIndicator', 0.75), 'low': _f(qt, 'badIndicator', 0.25)}
    return load_analysis_method(m)


def _fmt(value):
    if isinstance(value, bool):
        return 'true' if value else 'false'
    if isinstance(value, int):
        return str(value)
    return repr(float(value))


def to_gm_xml(method):
    """Write the HID subset as an ``AnalysisMethodContainer`` XML document."""
    m = load_analysis_method(method)
    a, p, q, s, f = m['allele'], m['peak_detector'], m['peak_quality'], m['sq_gq'], m['fixed']

    def el(parent, name, value=None):
        node = ET.SubElement(parent, name)
        if value is not None:
            node.text = value if isinstance(value, str) else _fmt(value)
        return node
    root = ET.Element('AnalysisMethodContainer', {'xmlns': NS, 'xmlns:xsi': 'http://www.w3.org/2001/XMLSchema-instance',
                                                  'xsi:type': 'AnalysisMethodContainer'})
    el(root, 'appVersion', '1.0'); el(root, 'fileVersion', '1.0')
    n = el(root, 'xmlAnalysisMethod')
    el(n, 'analysisMethodName', m['name']); el(n, 'originationProduct', 'OpenTrace')
    if m['description']:
        el(n, 'description', m['description'])
    if m['instrument']:
        el(n, 'instrument', m['instrument'])
    el(n, 'applicationMode', 2); el(n, 'signalProcessMode', 2)
    el(n, 'analysisFullRange', p['analysis_range']['full']); el(n, 'analysisStartPt', p['analysis_range']['start'])
    el(n, 'analysisStopPt', p['analysis_range']['stop'])
    el(n, 'sizeCallAllSizes', p['sizing_range']['all']); el(n, 'sizeCallStartSize', p['sizing_range']['start_bp'])
    el(n, 'sizeCallStopSize', p['sizing_range']['stop_bp']); el(n, 'smoothValue', SMOOTHING[p['smoothing']])
    for key, label in (('sq', 'Sizing Quality (SQ)'), ('gq', 'Genotype Quality (GQ)')):
        t = el(n, 'thresholdMap'); el(t, 'Key', label); qt = el(t, 'QualityThreshold')
        el(qt, 'goodIndicator', s[key]['pass']); el(qt, 'badIndicator', s[key]['low']); el(qt, 'name', label); el(qt, 'result', 'Pass')
    b = el(n, 'baselinerParameter'); el(b, 'AnalysisParameter'); el(b, 'windowSize', p['baseline_window']); el(b, 'spikeSize', 5)
    d = el(n, 'peakDetectorParameter'); el(d, 'AnalysisParameter')
    el(d, 'degree', p['polynomial_degree']); el(d, 'window', p['peak_window'])
    el(d, 'degreeTetri', 3); el(d, 'windowTetri', f['str_peak_window'])
    for dye, word in _XML_DYE.items():
        el(d, 'minHeight'+word, p['thresholds'][dye])
    el(d, 'minWidth', p['min_half_width']); el(d, 'tauB', p['slope_start']); el(d, 'tauE', p['slope_end'])
    el(d, 'heightAutoDetermined', False); el(d, 'applyNormalization', p['normalization'])
    sc = el(n, 'sizeCallerParameter'); el(sc, 'AnalysisParameter'); el(sc, 'sizeCallMethod', SIZE_CALLING[p['size_calling']])
    ac = el(n, 'alleleCallerParameter'); el(ac, 'AnalysisParameter')
    for repeat, prefix in _XML_REPEAT.items():
        r = a['repeats'][repeat]
        for suffix, key in (('cutOffValue', 'cutoff'), ('minusADistance', 'minus_a_to'), ('minusADistanceRight', 'minus_a_from'),
                            ('minusARatio', 'minus_a_ratio'), ('stutterDistance', 'minus_stutter_to'),
                            ('stutterDistanceRight', 'minus_stutter_from'), ('stutterRatio', 'minus_stutter_ratio'),
                            ('plus_stutterDistance', 'plus_stutter_to'), ('plus_stutterDistanceRight', 'plus_stutter_from'),
                            ('plus_stutterRatio', 'plus_stutter_ratio')):
            el(ac, f'{prefix}_{suffix}', r[key])
    el(ac, 'useMarkerStutterPercentage', a['use_marker_specific_stutter'])
    if a['bin_set']:
        el(ac, 'binSet', a['bin_set'])
    el(ac, 'amelogeninCutoff', a['amelogenin_cutoff'])
    el(ac, 'warningPeakHeightRatioThreshold', q['phr'])
    el(ac, 'warningPeakHeightThresholdHomozygous', q['lph_homozygous'])
    el(ac, 'warningPeakHeightThresholdHeterozygous', q['lph_heterozygous'])
    el(ac, 'warningHighPeakHeightThreshold', q['mph'])
    el(ac, 'warningMaxAlleleNumber', q['max_alleles']); el(ac, 'warningMaxAlleleNumberForYMarkers', q['max_alleles_y'])
    el(ac, 'warningPeakWidthThreshold', q['bd_bp'])
    el(ac, 'warningBleedthroughHeightThreshold', f['bleedthrough_height_ratio'])
    el(ac, 'warningBleedthroughScanThreshold', f['bleedthrough_scan_distance'])
    el(ac, 'spikeMaxPeakWidthThreshold', f['spike_max_width_bp'])
    el(ac, 'spikeLadderCutoffThreshold', q['ladder_spike']['cutoff'])
    el(ac, 'spikeDetectionEnabled', q['ladder_spike']['enabled'])
    el(ac, 'spikeDetectionForSampleEnabled', q['sample_spike'])
    for dye, word in _XML_DYE.items():
        el(ac, f'clearRange{word}DyeFrom', a['range_filter'][dye]['from'])
        el(ac, f'clearRange{word}DyeTo', a['range_filter'][dye]['to'])
    qv = el(n, 'qualityValueProcessParameter'); el(qv, 'AnalysisParameter')
    for key, name in (('bd', 'broadPeakQualityWeight'), ('bin', 'binningQualityWeight'), ('lph', 'peakAbsoluteHeightQualityWeight'),
                      ('mph', 'highPeakHeightQualityWeight'), ('spk', 'spikeMarkerPqvQualityWeight'),
                      ('an', 'alleleNumberMarkerPqvQualityWeight'), ('phr', 'peakHeightRatioQualityWeight'),
                      ('os', 'offscaleQualityWeight'), ('cc', 'controlAlleleQualityWeight'), ('ovl', 'overlapQualityWeight'),
                      ('acc', 'amelCCQualityWeight')):
        el(qv, name, s['weights'][key])
    el(qv, 'BroadPeakSQPqvQualityWeight', s['sq_weight'])
    el(qv, 'profilePullupQualityThreshold', f['profile_pullup_threshold'])
    el(qv, 'alSpikeMarkerPqvQualityWeight', float(s['ladder_spike']))
    el(qv, 'alOffscaleMarkerPqvQualityWeight', float(s['ladder_offscale']))
    body = ET.tostring(root, encoding='unicode')
    return '<?xml version="1.0" encoding="UTF-8"?>\n' + body


def method_request(request):
    """HTTP helper for /api/analysis-method."""
    if not isinstance(request, dict):
        raise ValueError('Analysis method request must be an object')
    action = request.get('action')
    if action == 'factory':
        return {'method': factory_method(request.get('name'))}
    if action == 'validate':
        method = load_analysis_method(request.get('method'))
        return {'method': method, 'unsupported': unsupported(method)}
    if action == 'import_xml':
        method = parse_gm_xml(request.get('xml'))
        return {'method': method, 'unsupported': unsupported(method)}
    if action == 'export_xml':
        return {'xml': to_gm_xml(request.get('method'))}
    if action == 'analyze_settings':
        return {'settings': analyze_settings(request.get('method'), request.get('standard_bp') or [],
                                             request.get('dye_channels'))}
    raise ValueError('action must be factory, validate, import_xml, export_xml or analyze_settings')
