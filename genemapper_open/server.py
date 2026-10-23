"""Loopback-only research UI; no file persistence and no external services."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import hashlib
import json
import math
from numbers import Real
from pathlib import Path
import re
import secrets
import urllib.parse
import webbrowser

from .abif import AbifError, AbifFile, MAX_FILE_BYTES
from .analysis import find_peaks, parse_anchors
from .demo import demo_bytes, sizing_demo_bytes, pipeline_demo_bytes

from . import __version__

ANALYSIS_VERSION = "OpenTrace/" + __version__


def _pipeline_expected(value):
    if isinstance(value, str):
        if len(value) > 4096:
            raise ValueError("Expected fragment text is too long")
        try:
            value = [float(x) for x in re.split(r"[,;\s]+", value.strip(), maxsplit=32)]
        except ValueError as exc:
            raise ValueError("Expected fragment sizes must be numbers") from exc
    if not isinstance(value, (list, tuple)) or not 3 <= len(value) <= 32:
        raise ValueError("Enter 3..32 explicit increasing fragment sizes")
    if any(not isinstance(x, Real) or isinstance(x, bool) or not math.isfinite(x)
           or not 0 < x <= 1_000_000 for x in value):
        raise ValueError("Expected fragment sizes must be finite and in (0, 1000000]")
    sizes = [float(x) for x in value]
    if any(b-a < 1e-6 for a, b in zip(sizes, sizes[1:])):
        raise ValueError("Expected fragment sizes must increase with gaps >=0.000001bp")
    return sizes


def _public_sg_peaks(peaks, raw_values):
    return [dict(p, raw_height=raw_values[int(p['scan'])], width_scans=p['fwhm_scan']) for p in peaks]


def _measure_morphology_channel(channel, threshold,spike_definition=None):
    from .preprocessing import prepare_trace
    from .sg_peaks import detect_sg_peaks
    from .standard_wrapper import wrap_standard_peaks
    if len(channel['values']) < 9:
        channel['peaks'] = []
        channel['processing'] = {'status': 'skipped', 'reason': 'Trace is shorter than SG window9'}
        return
    prepared = prepare_trace(channel['values'], window=51, spike_window=5, smooth_passes=2)
    detected = detect_sg_peaks(prepared['signal'], threshold=threshold, window=9)
    if spike_definition is not None:
        from .spike import measure_spike_peaks
        measurement=prepared['corrected'] if spike_definition['schema_version']>=3 else prepared['signal']
        features=measure_spike_peaks(measurement,detected['peaks'],spike_definition)
        for peak,feature in zip(detected['peaks'],features):peak['spike_features']=feature
    projected = wrap_standard_peaks(detected['peaks'], signal_length=len(channel['values']))
    channel['peaks'] = _public_sg_peaks(projected['peaks'], channel['values'])
    # The prepared detector input (baseline-corrected, smoothed) for display;
    # raw values stay unchanged for the raw-data view.
    channel['analyzed_values'] = prepared['signal']
    channel['processing'] = {'status': 'measured', 'preparation': prepared['parameters'],
        'detector': detected['diagnostics'], 'coordinate_conversion': projected['diagnostics'],
        'height_mode': 'explicit', 'original_runtime_validated': False}


def analyze(raw, filename, threshold=100.0, distance=8, anchors="", *,
            sizing_mode="", standard_channel="", standard_bp="", max_residual_bp=5.0,
            standard_start=0, standard_end=None, calibration_model="linear",
            analysis_engine="residual", height_mode="explicit", quality_threshold=0.75,
            num_extra_peaks=25,spike_definition=None,sizing_quality_definition=None,pullup_definition=None):
    from .calibration import MODEL_IDS, build_calibration, CalibrationError
    if calibration_model not in MODEL_IDS:
        raise ValueError("Unknown calibration model")
    if analysis_engine not in ('residual', 'morphology_ratio'):
        raise ValueError("analysis_engine must be residual or morphology_ratio")
    if spike_definition is not None:
        from .spike import load_spike_definition
        spike_definition=load_spike_definition(spike_definition)
        if analysis_engine!='morphology_ratio':raise ValueError('Basic spike classification requires morphology_ratio engine')
    if sizing_quality_definition is not None:
        from .post_sizing_quality import load_sizing_quality_definition
        sizing_quality_definition=load_sizing_quality_definition(sizing_quality_definition)
        if analysis_engine!='morphology_ratio':raise ValueError('Post-sizing quality requires morphology_ratio engine')
    if pullup_definition is not None:
        from .pullup import load_pullup_definition
        pullup_definition=load_pullup_definition(pullup_definition)
        if analysis_engine!='morphology_ratio':raise ValueError('Explicit pull-up route requires morphology_ratio engine')
    if analysis_engine == 'morphology_ratio':
        if height_mode not in ('explicit', 'basic_standard_auto'):
            raise ValueError("height_mode must be explicit or basic_standard_auto")
        if (not isinstance(quality_threshold, Real) or isinstance(quality_threshold, bool)
                or not math.isfinite(quality_threshold) or not 0 < quality_threshold <= 1):
            raise ValueError("quality_threshold must be finite and in (0, 1]")
        if type(num_extra_peaks) is not int or not 0 <= num_extra_peaks <= 32:
            raise ValueError("num_extra_peaks must be an integer in [0, 32]")
        if not isinstance(threshold, Real) or isinstance(threshold, bool) or not math.isfinite(threshold) or not 0 < threshold <= 1e12:
            raise ValueError("Morphology peak threshold must be finite and in (0, 1e12]")
    sample = AbifFile(raw, filename)
    mode = sizing_mode or ("auto" if standard_channel or standard_bp else "manual" if anchors.strip() else "none")
    if mode not in ("none", "manual", "auto"):
        raise ValueError("Sizing mode must be none, manual or auto")
    if spike_definition is not None and mode!='auto':
        raise ValueError('Initial spike subset requires an explicit automatic reference dye')
    if sizing_quality_definition is not None and mode!='auto':
        raise ValueError('Post-sizing quality requires explicit automatic reference dye and fragments')
    if pullup_definition is not None and mode!='auto':raise ValueError('Pull-up policy requires an explicit automatic reference dye')
    if mode == "auto" and anchors.strip():
        raise ValueError("Automatic sizing cannot be combined with manual anchors")
    calibration = parse_anchors(anchors) if mode == "manual" else None
    if mode == "manual" and calibration is None:
        raise ValueError("Manual sizing requires at least two scan:bp anchors")
    channels = sample.channels()
    if not channels:
        raise AbifError("No supported numeric DATA traces found; unknown DATA types remain opaque")
    for ch in channels:
        if any(not math.isfinite(value) for value in ch["values"]):
            raise AbifError(f"{ch['tag']}: non-finite numeric values")
        if analysis_engine == 'residual':
            ch["peaks"] = find_peaks(ch["values"], threshold, distance)
    reference = None
    standard_selected=[]
    expected = None
    if mode == 'auto':
        if not math.isfinite(max_residual_bp) or not 0 < max_residual_bp <= 100:
            raise ValueError("Maximum sizing residual must be finite and in (0, 100] bp")
        candidates = [ch for ch in channels if ch['tag'] == standard_channel]
        if not candidates or candidates[0]['group'] == 'auxiliary':
            raise ValueError("Select an existing raw/analyzed dye channel for the size standard")
        reference = candidates[0]
        end = len(reference['values'])-1 if standard_end is None else standard_end
        if (type(standard_start) is not int or type(end) is not int
                or not 0 <= standard_start < end < len(reference['values'])):
            raise ValueError("Standard scan range must be within the selected reference trace")
        if analysis_engine == 'morphology_ratio':
            if standard_start != 0 or end != len(reference['values'])-1:
                raise ValueError("Morphology/ratio engine requires the full reference trace; partial ranges are not supported")
            expected = _pipeline_expected(standard_bp)
        else:
            from .sizing import parse_expected_bp
            expected = parse_expected_bp(standard_bp) if isinstance(standard_bp, str) else standard_bp
    selected_group = None
    if analysis_engine == 'morphology_ratio':
        selected_group = reference['group'] if reference else 'raw' if any(ch['group'] == 'raw' for ch in channels) else 'analyzed'
        for ch in channels:
            if ch['group'] != selected_group:
                ch['peaks'] = []
                ch['processing'] = {'status': 'skipped', 'reason': 'Auxiliary or a different DATA group; no cross-group sizing'}
            elif ch is not reference:
                _measure_morphology_channel(ch, threshold,spike_definition)
    sizing = {"mode": mode, "status": "manual" if calibration else "not_requested",
              "reasons": [], "matched": [], "unmatched": [], "diagnostics": {}}
    if mode == "auto":
        if analysis_engine == 'residual':
            from .sizing import match_size_standard
            matched = match_size_standard(
                [p for p in reference["peaks"] if standard_start <= p["scan"] <= end],
                expected, max_residual_bp=max_residual_bp)
            calibration = matched.pop("calibration", None)
            sizing = matched
        else:
            from .sizing_pipeline import analyze_size_standard
            result = analyze_size_standard(reference['values'], expected,
                num_extra_peaks=num_extra_peaks, quality_threshold=quality_threshold,
                detection_threshold=threshold, height_mode=height_mode, coordinate_mode='standard_wrapper')
            calibration = result['calibration']
            standard_selected=result['selected_peaks']
            reference['peaks'] = _public_sg_peaks(result['peaks'], reference['values'])
            reference['processing'] = {key: result[key] for key in ('preparation', 'detector', 'coordinate_conversion', 'autoheight')}
            matching = result['matching'] or {}
            filtered = result['filtering'] or {}
            sizing = {'status': result['status'], 'stage': result['stage'], 'reasons': result['reasons'],
                'matched': [dict(p, holdout_error_bp=None) for p in matching.get('matched', [])],
                'unmatched': [dict(p) for p in result['selected_peaks'] if p.get('matched_bp') is None],
                'candidate_count': len(result['peaks']), 'expected_count': len(expected),
                'diagnostics': matching.get('diagnostics', {}), 'matching': matching,
                'base_quality': result['base_quality'], 'autoheight': result['autoheight'],
                'preparation': result['preparation'], 'detector': result['detector'],
                'coordinate_conversion': result['coordinate_conversion'],
                'prefilter': {key: filtered.get(key) for key in ('status', 'reasons', 'diagnostics')},
                'settings': {'height_mode': height_mode, 'quality_threshold': quality_threshold,
                    'num_extra_peaks': num_extra_peaks, 'coordinate_mode': 'standard_wrapper',
                    'ratio_factor': 0.8, 'ratio_score_threshold': 0.75, 'ambiguity_margin': 0.01,
                    'max_bp_per_scan': 0.25, 'selected_data_group': selected_group},
                'not_applied': result['not_applied'], 'external_validation': False,
                'original_runtime_validated': False}
        sizing.update(mode='auto', reference_channel=standard_channel,
                      scan_range=[standard_start, end], expected_bp=list(expected),
                      max_residual_bp=max_residual_bp)
    sizing['analysis_engine'] = analysis_engine
    sizing["calibration_model"] = calibration_model
    sizing["calibration_diagnostics"] = {}
    if calibration:
        try:
            curve = build_calibration(calibration.anchors, calibration_model)
            sizing["calibration_diagnostics"] = curve.diagnostics
            if mode == "auto" and curve.diagnostics["max_abs_anchor_error_bp"] > max_residual_bp:
                raise CalibrationError("Calibration fit exceeds the permitted anchor error")
            calibration = curve
        except CalibrationError as exc:
            if mode != "auto":
                raise
            sizing["status"] = "rejected"
            sizing["reasons"].append("Calibration model rejected: " + str(exc))
            sizing['stage'] = 'calibration_model'
            calibration = None
    if calibration:
        for ch in channels:
            for peak in ch["peaks"]:
                peak["size_bp"] = calibration.size(peak["scan"])
    sizing["anchors"] = calibration.anchors if calibration else []
    if mode == 'auto':
        for matched in sizing['matched']:
            matched['tentative'] = sizing['status'] != 'accepted'
        sizing['calibration_applied'] = calibration is not None
    if sizing_quality_definition is not None:
        from .post_sizing_quality import evaluate_post_sizing_quality
        sizing['post_quality']=evaluate_post_sizing_quality(sizing.get('base_quality'),standard_selected,
            calibration,sizing_quality_definition,reference_channel=standard_channel)
        if sizing['post_quality']['status']=='evaluated':
            sizing['not_applied']=[s for s in sizing.get('not_applied',[]) if s!='ay broad-width multiplier']
    artifacts=None
    if spike_definition is not None:
        from .spike import classify_spike
        from .quality_control import dimensions_in_bp
        artifacts={'status':'evaluated_subset','classified':0,'current_spikes':0,'overlaps':0,'unavailable':0,
            'scope':'initial_non_reference_classification','reference_channel_unchanged':True,
            'downstream_spike_adjustments_applied':False,'automatic_pullup_applied':False}
        if spike_definition['schema_version']>=3:
            artifacts.update(morphology_measurement_source='baseline_corrected_companion',
                morphology_scope='jag_and_ideal_gaussian_fit_only',true_peak_restoration_applied=False,poor_fit_peaks=0)
        if spike_definition['schema_version']==4:
            artifacts.update(morphology_scope='side_collection_true_peak_and_fresh_restoration',
                restored_peaks=0,height_only_peaks=0,restoration_unavailable=0)
        for ch in channels:
            if ch['group']!=selected_group or ch is reference:continue
            for peak in ch['peaks']:
                features=peak.get('spike_features')
                if features and features['status']=='evaluated_subset' and calibration:
                    dimensions=dimensions_in_bp({'begin_scan':peak['begin'],'end_scan':peak['end'],
                        'fwhm_scan':peak['fwhm_scan'],'area':peak['area']},calibration)
                    if dimensions['status']=='evaluated':
                        scale=dimensions['bp_per_scan_span']
                        for name in ('exhalf_width','near_base_width'):
                            features[name+'_bp']=features[name+'_scan']*scale if features[name+'_scan']>=0 else -1
                        features['bp_per_scan_span']=scale
                classified=classify_spike(features,height=peak['height'],offscaled=peak['offscaled'],definition=spike_definition)
                if spike_definition['schema_version']==4:
                    from .true_peak import restore_peak
                    restored=restore_peak(peak,classified,calibration=calibration,signal_length=len(ch['values']))
                    peak.update(restored)
                    history=peak['true_peak_restoration']
                    classified['true_peak_restoration_applied']=bool(history['writes'])
                    artifacts['true_peak_restoration_applied']|=bool(history['writes'])
                    artifacts['restored_peaks']+=int(history['status']=='restored')
                    artifacts['height_only_peaks']+=int(history['status']=='height_only')
                    artifacts['restoration_unavailable']+=int(history['status']=='unavailable')
                peak['spike_classification']=classified
                if spike_definition['schema_version']>=3:
                    artifacts['poor_fit_peaks']+=int(classified.get('peak_deformity')=='poor_fit')
                if classified['status']=='evaluated_subset':
                    peak['spike']=classified['current_spike'];artifacts['classified']+=1
                    artifacts['current_spikes']+=int(classified['current_spike']);artifacts['overlaps']+=int(classified['overlap_spike'])
                else:artifacts['unavailable']+=1
        if artifacts['unavailable']:artifacts['status']='partial'
    pullup_detection=None
    if pullup_definition is not None:
        from .pullup import apply_pullup_detection
        detection=apply_pullup_detection(channels,pullup_definition,selected_group=selected_group,reference_channel=standard_channel)
        for channel in channels:
            if channel['tag'] in detection['peaks_by_channel']:channel['peaks']=detection['peaks_by_channel'][channel['tag']]
        pullup_detection=detection['summary']
        if pullup_definition['detector'] is not None:
            if artifacts is None:artifacts={'status':'evaluated_subset','classified':0,'current_spikes':0,'overlaps':0,
                'unavailable':0,'reference_channel_unchanged':True,'downstream_spike_adjustments_applied':False}
            artifacts['automatic_pullup_applied']=True
            artifacts['pullup_routing_scope']='explicit_experimental_reconstruction_original_route_unconfirmed'
    output=sample.summary() | {"channels": channels, "directory": sample.directory(),
                               "calibration": calibration.anchors if calibration else [],
                               "sizing": sizing, "source_sha256": hashlib.sha256(raw).hexdigest(),
                               "analysis_version": ANALYSIS_VERSION,
                               "peak_settings": {"threshold": threshold, "min_distance": distance if analysis_engine == 'residual' else None,
                                  'analysis_engine': analysis_engine, 'selected_data_group': selected_group,
                                  'height_mode': height_mode if analysis_engine == 'morphology_ratio' else 'explicit'},
                               "research_only": True}
    if artifacts is not None:output['artifacts']=artifacts
    if spike_definition is not None:
        output['peak_settings']['spike_definition']=spike_definition
        output['artifacts']=artifacts
    if sizing_quality_definition is not None:
        output['peak_settings']['sizing_quality_definition']=sizing_quality_definition
    if pullup_definition is not None:
        output['peak_settings']['pullup_definition']=pullup_definition
        output['pullup_detection']=pullup_detection
    return output


def serve(port=8765, open_browser=False):
    token = secrets.token_urlsafe(24)
    page = (Path(__file__).parent / "web" / "index.html").read_text(encoding="utf-8")
    page = page.replace("__SESSION_TOKEN__", token).encode("utf-8")
    # Desktop-style workspace: a fixed whitelist of static files, no path lookup.
    idx_dir = Path(__file__).parent / "web" / "idx"
    idx_types = {".html": "text/html; charset=utf-8", ".css": "text/css; charset=utf-8",
                 ".js": "text/javascript; charset=utf-8"}
    idx_files = {name: (idx_dir / name).read_bytes() for name in
                 ("project.html", "plot.html", "panels.html", "idx.css", "i18n.js", "ui.js", "kits.js",
                  "project.js", "plot.js", "panels.js")}
    for page_name in ("project.html", "panels.html"):
        idx_files[page_name] = idx_files[page_name].replace(b"__SESSION_TOKEN__", token.encode("ascii"))

    class Handler(BaseHTTPRequestHandler):
        server_version = "OpenTrace/" + __version__

        def respond(self, code, body, kind="application/json; charset=utf-8"):
            if isinstance(body, (dict, list)):
                body = json.dumps(body, ensure_ascii=False, allow_nan=False).encode("utf-8")
            self.send_response(code)
            self.send_header("Content-Type", kind)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self' 'unsafe-inline'; "
                             "style-src 'self' 'unsafe-inline'; connect-src 'self'; object-src 'none'; frame-ancestors 'none'")
            self.end_headers()
            self.wfile.write(body)

        def valid_host(self):
            return self.headers.get("Host") == f"127.0.0.1:{self.server.server_port}"

        def do_GET(self):
            if not self.valid_host():
                return self.respond(403, {"error": "Use the 127.0.0.1 URL printed by the server"})
            if self.path == "/":
                return self.respond(200, page, "text/html; charset=utf-8")
            if self.path in ("/idx", "/idx/"):
                return self.respond(200, idx_files["project.html"], idx_types[".html"])
            if self.path.startswith("/idx/") and self.path[5:] in idx_files:
                name = self.path[5:]
                return self.respond(200, idx_files[name], idx_types[Path(name).suffix])
            if self.path == "/favicon.ico":
                return self.respond(204, b"", "image/x-icon")
            if self.path == "/api/health":
                return self.respond(200, {"status": "ok", "analysis_version": ANALYSIS_VERSION, "research_only": True})
            return self.respond(404, {"error": "Not found"})

        def do_POST(self):
            if not self.valid_host() or self.headers.get("X-Session-Token") != token:
                return self.respond(403, {"error": "Invalid local session"})
            origin = self.headers.get("Origin")
            if origin and origin != f"http://127.0.0.1:{self.server.server_port}":
                return self.respond(403, {"error": "Invalid origin"})
            parsed = urllib.parse.urlsplit(self.path)
            if parsed.path not in ("/api/analyze", "/api/demo", "/api/demo-sizing", "/api/demo-pipeline",
                                   "/api/str", "/api/demo-str", "/api/str-run", "/api/demo-str-run", "/api/project", "/api/controls", "/api/marker-quality", "/api/pullup-quality", "/api/import-gm-kit"):
                return self.respond(404, {"error": "Not found"})
            try:
                length = int(self.headers.get("Content-Length", "0"))
                if length < 0 or length > MAX_FILE_BYTES:
                    return self.respond(413, {"error": "Maximum upload is 64 MiB"})
                params = urllib.parse.parse_qs(parsed.query)
                self.connection.settimeout(30)
                raw = self.rfile.read(length)
                if len(raw) != length:
                    raise ValueError("Incomplete request body")
                if parsed.path == '/api/project':
                    from .project_api import project_action
                    return self.respond(200,project_action(json.loads(raw)))
                if parsed.path == '/api/controls':
                    from .control_workflow import attach_control_result
                    request=json.loads(raw)
                    if not isinstance(request,dict):raise ValueError('Control request must be a JSON object')
                    return self.respond(200,attach_control_result(request.get('report'),request.get('definition')))
                if parsed.path == '/api/marker-quality':
                    from .marker_quality_workflow import attach_marker_quality
                    request=json.loads(raw)
                    if not isinstance(request,dict):raise ValueError('Marker quality request must be an object')
                    return self.respond(200,attach_marker_quality(request.get('report'),request.get('definition')))
                if parsed.path == '/api/import-gm-kit':
                    from .gm_kit_import import import_request
                    return self.respond(200,import_request(json.loads(raw)))
                if parsed.path == '/api/pullup-quality':
                    from .pullup_quality import attach_pullup_quality
                    request=json.loads(raw)
                    if not isinstance(request,dict):raise ValueError('Pull-up quality request must be an object')
                    return self.respond(200,attach_pullup_quality(request.get('report'),request.get('definition')))
                if parsed.path == '/api/str':
                    from .str_workflow import run_str_workflow
                    request=json.loads(raw)
                    if not isinstance(request,dict):
                        raise ValueError('STR request must be a JSON object')
                    return self.respond(200,run_str_workflow(request.get('sample'),request.get('ladder'),
                        request.get('kit'),missing_offset_policy=request.get('missing_offset_policy','reject'),
                        ladder_quality_definition=request.get('ladder_quality_definition')))
                if parsed.path == '/api/str-run':
                    from .run_workflow import run_str_run_workflow
                    request=json.loads(raw)
                    if not isinstance(request,dict):raise ValueError('Run STR request must be an object')
                    return self.respond(200,run_str_run_workflow(request.get('sample'),request.get('ladders'),
                        request.get('kit'),ladder_quality_definition=request.get('ladder_quality_definition'),
                        missing_offset_policy=request.get('missing_offset_policy','reject')))
                if parsed.path == '/api/demo-str-run':
                    from .str_demo import make_run_demo
                    from .str_workflow import compact_analysis
                    from .run_workflow import run_str_run_workflow
                    fixture=make_run_demo();sample=analyze(fixture['sample_bytes'],'invented-run-sample.fsa',**fixture['settings'])
                    ladders=[{'ladder_id':r['ladder_id'],'analysis':compact_analysis(analyze(r['data'],r['ladder_id'],**fixture['settings']))}
                        for r in fixture['ladders']]
                    report=run_str_run_workflow(sample,ladders,fixture['kit'],
                        ladder_quality_definition=fixture['ladder_quality_definition'])
                    return self.respond(200,{'sample':sample,'ladders':ladders,'kit':fixture['kit'],'report':report,
                        'ladder_quality_definition':fixture['ladder_quality_definition'],
                        'marker_quality_definition':fixture['marker_quality_definition']})
                if parsed.path == '/api/demo-str':
                    from .str_demo import (make_str_demo,make_quality_demo_definition,
                                           make_cross_check_demo,make_negative_demo_definitions,
                                           make_ladder_quality_demo_definition,make_spike_demo_definition,make_spike_adjustment_demo,make_post_sizing_demo,make_ladder_calling_demo,make_morphology_demo,make_true_peak_demo)
                    from .str_workflow import run_str_workflow, compact_analysis
                    variant=params.get('variant',['ordinary'])[0]
                    if variant not in ('ordinary','negative','amel','ladder','ladder-rejected','spike','spike-unrequested','spike-low-initial','spike-low-adjusted','sq-pass','sq-check','sq-fail','sq-ladder-fail','ladder-calls','ladder-sample-filters','morphology','morphology-unrequested','true-peak','true-peak-excluded','pullup','pullup-flags-only','pullup-unrequested'):raise ValueError('Unknown invented STR demo variant')
                    controls=None
                    ladder_quality_definition=(make_ladder_quality_demo_definition(reject=variant=='ladder-rejected')
                        if variant in ('ladder','ladder-rejected') else None)
                    if variant=='amel':fixture,quality_definition=make_cross_check_demo()
                    else:
                        fixture=make_str_demo(include_spike=variant in ('spike','spike-unrequested'));quality_definition=make_quality_demo_definition()
                        if variant=='negative':quality_definition,controls=make_negative_demo_definitions()
                        if variant=='spike':fixture['settings']['spike_definition']=make_spike_demo_definition()
                        if variant in ('spike-low-initial','spike-low-adjusted'):
                            fixture=make_spike_adjustment_demo(adjusted=variant=='spike-low-adjusted')
                        if variant in ('morphology','morphology-unrequested'):
                            fixture=make_morphology_demo(enabled=variant=='morphology')
                        if variant in ('true-peak','true-peak-excluded'):
                            fixture=make_true_peak_demo(exclude_overlap=variant=='true-peak-excluded')
                        if variant.startswith('pullup'):
                            from .str_demo import make_pullup_demo
                            fixture=make_pullup_demo(detector=variant=='pullup',requested=variant!='pullup-unrequested')
                        if variant.startswith('sq-'):
                            fixture=make_post_sizing_demo(variant);quality_definition['sample_sq']={'source':'post_sizing_quality'}
                            if variant=='sq-ladder-fail':
                                ladder_quality_definition=make_ladder_quality_demo_definition()
                                ladder_quality_definition['ladder_sq']={'source':'post_sizing_quality'}
                        if variant in ('ladder-calls','ladder-sample-filters'):
                            fixture,ladder_quality_definition=make_ladder_calling_demo(legacy=variant=='ladder-sample-filters')
                    sample=analyze(fixture['sample_bytes'],'invented-sample.fsa',**fixture['settings'])
                    ladder=analyze(fixture['ladder_bytes'],'invented-ladder.fsa',**fixture['settings'])
                    report=run_str_workflow(sample,ladder,fixture['kit'],ladder_quality_definition=ladder_quality_definition)
                    report['provenance']['input_reports_are_caller_supplied']=False
                    if controls:
                        from .control_workflow import attach_control_result
                        report=attach_control_result(report,controls)
                    if variant in ('negative','amel','spike','spike-unrequested','spike-low-initial','spike-low-adjusted','ladder-calls','ladder-sample-filters','morphology','morphology-unrequested','true-peak','true-peak-excluded','pullup','pullup-flags-only','pullup-unrequested') or variant.startswith('sq-'):
                        from .marker_quality_workflow import attach_marker_quality
                        report=attach_marker_quality(report,quality_definition)
                    return self.respond(200,{'sample':sample,'ladder':compact_analysis(ladder),
                        'kit':fixture['kit'],'report':report,'marker_quality_definition':quality_definition,
                        'control_definition':controls,'demo_variant':variant,
                        'ladder_quality_definition':ladder_quality_definition,
                        'sizing_quality_definition':fixture['settings'].get('sizing_quality_definition'),
                        'pullup_definition':fixture['settings'].get('pullup_definition'),
                        'spike_definition':fixture['settings'].get('spike_definition',
                            make_spike_demo_definition() if variant=='spike-unrequested' else None)})
                demo = {'/api/demo': demo_bytes, '/api/demo-sizing': sizing_demo_bytes,
                        '/api/demo-pipeline': pipeline_demo_bytes}.get(parsed.path)
                data = demo() if demo else raw
                result = analyze(data,
                                 params.get("filename", ["synthetic.fsa"])[0],
                                 float(params.get("threshold", ["100"])[0]),
                                 int(params.get("distance", ["8"])[0]),
                                 params.get("anchors", [""])[0],
                                 sizing_mode=params.get("sizing_mode", [""])[0],
                                 standard_channel=params.get("standard_channel", [""])[0],
                                 standard_bp=params.get("standard_bp", [""])[0],
                                 max_residual_bp=float(params.get("max_residual_bp", ["5"])[0]),
                                 standard_start=int(params.get("standard_start", ["0"])[0]),
                                 standard_end=int(params["standard_end"][0]) if params.get("standard_end") else None,
                                 calibration_model=params.get("calibration_model", ["linear"])[0],
                                 analysis_engine=params.get("analysis_engine", ["residual"])[0],
                                 height_mode=params.get("height_mode", ["explicit"])[0],
                                 quality_threshold=float(params.get("quality_threshold", ["0.75"])[0]),
                                 num_extra_peaks=int(params.get("num_extra_peaks", ["25"])[0]),
                                 spike_definition=params.get('spike_definition',[None])[0],
                                 sizing_quality_definition=params.get('sizing_quality_definition',[None])[0],
                                 pullup_definition=params.get('pullup_definition',[None])[0])
                return self.respond(200, result)
            except (AbifError, ValueError, OverflowError, TimeoutError, TypeError, KeyError, RecursionError) as exc:
                return self.respond(400, {"error": str(exc)})

    class LocalServer(ThreadingHTTPServer):
        # Avoid Windows allowing two process instances to share one port.
        allow_reuse_address = False

    server = LocalServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    url = f"http://127.0.0.1:{server.server_port}/"
    print(f"OpenTrace research UI: {url}\nDesktop-style workspace: {url}idx/\n"
          "Ctrl+C to stop. No uploaded files are saved.", flush=True)
    if open_browser:
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
