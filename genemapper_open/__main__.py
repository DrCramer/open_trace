import argparse
import json
from pathlib import Path
import sqlite3
import sys
import tempfile

from .abif import AbifError, AbifFile
from .demo import demo_bytes, sizing_demo_bytes, pipeline_demo_bytes
from .export import peaks_csv, traces_csv
from .server import analyze, serve
from .project_store import MAX_EXPORT_BYTES, ProjectStore


_PROJECT_RESULT_FIELDS = (
    "schema_version", "source_sha256", "analysis_version", "status", "calls", "peaks",
    "sizing", "quality", "qc", "warnings", "ladder", "allele_calling", "kit_identity",
    "sample_identity", "ladder_identity", "provenance", "research_only",
)


def _read_json_text(path, limit=MAX_EXPORT_BYTES):
    if path.stat().st_size > limit:
        raise ValueError(f"JSON file exceeds {limit} byte limit")
    return path.read_text(encoding="utf-8")


def _project_parser(commands):
    project = commands.add_parser("project", help="Local research projects and append-only review")
    actions = project.add_subparsers(dest="project_action", required=True)
    create = actions.add_parser("create", help="Create a new SQLite project")
    create.add_argument("database", type=Path)
    create.add_argument("--name", required=True)
    for action in ("list", "show"):
        command = actions.add_parser(action, help="List project records" if action == "list" else "Show effective review and machine result")
        command.add_argument("database", type=Path)
        if action == "show":
            command.add_argument("--analysis-id", help="Show one analysis with its separate review overlay")
    save = actions.add_parser("save", help="Save a compact analysis JSON report, excluding raw channel arrays")
    save.add_argument("database", type=Path)
    save.add_argument("report", type=Path)
    save.add_argument("--sample-id", help="Use an existing sample with the same source SHA-256")
    save.add_argument("--status", help="Explicit status when absent from the report; accepted must come from JSON")
    export_project = actions.add_parser("export", help="Export versioned JSON to a new file")
    export_project.add_argument("database", type=Path)
    export_project.add_argument("output", type=Path)
    import_project = actions.add_parser("import", help="Import versioned JSON into a new database")
    import_project.add_argument("input", type=Path)
    import_project.add_argument("database", type=Path)
    review = actions.add_parser("review", help="Append an attributed review event")
    review.add_argument("database", type=Path)
    review.add_argument("analysis_id")
    review.add_argument("action", choices=("accept", "exclude", "change-label", "note",
                                           "add-allele", "delete-artifact-label", "override-gq", "override-cgq"))
    review.add_argument("--target-kind", choices=("analysis", "call", "peak", "marker"), default="analysis")
    review.add_argument("--target-id")
    review.add_argument("--value", help="Required text for change-label, note or add-allele (peak target)")
    undo = actions.add_parser("undo", help="Append a new event reversing an earlier review event")
    undo.add_argument("database", type=Path)
    undo.add_argument("analysis_id")
    undo.add_argument("event_id")
    for command in (review, undo):
        command.add_argument("--actor", required=True, help="Caller-provided attribution; no authentication")
        command.add_argument("--reason", required=True)
    upgrade=actions.add_parser('upgrade',help='Copy a project into the current schema without changing its source')
    upgrade.add_argument('database',type=Path);upgrade.add_argument('output',type=Path)
    for name in ('run-create','run-revise','run-replace-ladder','run-remove-ladder','run-show'):
        command=actions.add_parser(name,help='Persistent run revisions and dependent sample results')
        command.add_argument('database',type=Path)
        if name!='run-create':command.add_argument('run_id')
        if name=='run-create':
            command.add_argument('inputs',type=Path);command.add_argument('--name',required=True)
        if name=='run-revise':command.add_argument('inputs',type=Path)
        if name in ('run-replace-ladder','run-remove-ladder'):command.add_argument('ladder_id')
        if name=='run-replace-ladder':command.add_argument('analysis',type=Path,help='Compact sized replacement report JSON')
        if name!='run-show':
            command.add_argument('--actor',required=True);command.add_argument('--reason',required=True)
            if name!='run-create':command.add_argument('--parent-revision',required=True)


def _str_run_command(args):
    from .run_workflow import analyze_str_run_files
    if args.command=='str-run-demo':
        from .str_demo import make_run_demo
        fixture=make_run_demo();args.directory.mkdir(parents=False,exist_ok=False)
        (args.directory/'sample.fsa').write_bytes(fixture['sample_bytes'])
        for row in fixture['ladders']:(args.directory/row['ladder_id']).write_bytes(row['data'])
        documents={'kit.json':fixture['kit'],'settings.json':fixture['settings'],
            'ladder-quality.json':fixture['ladder_quality_definition'],'marker-quality.json':fixture['marker_quality_definition']}
        manifest={'schema_version':1,'sample':'sample.fsa','kit':'kit.json','settings':'settings.json',
            'ladder_quality':'ladder-quality.json','missing_offset_policy':'reject',
            'ladders':[{'ladder_id':r['ladder_id'],'file':r['ladder_id']} for r in fixture['ladders']]}
        documents['run.json']=manifest
        for name,data in documents.items():
            (args.directory/name).write_text(json.dumps(data,ensure_ascii=False,allow_nan=False,indent=2)+'\n',encoding='utf-8')
        report=analyze_str_run_files(fixture['sample_bytes'],fixture['ladders'],fixture['kit'],settings=fixture['settings'],
            ladder_quality_definition=fixture['ladder_quality_definition'])
        (args.directory/'report.json').write_text(json.dumps(report,ensure_ascii=False,allow_nan=False,indent=2)+'\n',encoding='utf-8')
        print(args.directory.resolve());return report
    manifest=json.loads(_read_json_text(args.manifest,262144))
    fields={'schema_version','sample','kit','settings','ladder_quality','missing_offset_policy','ladders'}
    if not isinstance(manifest,dict) or set(manifest)!=fields or type(manifest['schema_version']) is not int or manifest['schema_version']!=1:
        raise ValueError('Explicit version1 run manifest with all fields required')
    root=args.manifest.resolve().parent
    def path(value):
        if not isinstance(value,str) or not value:raise ValueError('Nonempty run input path required')
        return root/value
    def data(value):
        source=path(value)
        if source.stat().st_size>64*1024*1024:raise ValueError('Run FSA exceeds64MiB')
        return source.read_bytes()
    rows=manifest['ladders']
    if not isinstance(rows,list) or not 1<=len(rows)<=128:raise ValueError('Explicit1..128 run ladders required')
    ladders=[]
    for row in rows:
        if not isinstance(row,dict) or set(row)!={'ladder_id','file'}:raise ValueError('Run ladder needs ladder_id/file')
        ladders.append({'ladder_id':row['ladder_id'],'data':data(row['file'])})
    report=analyze_str_run_files(data(manifest['sample']),ladders,_read_json_text(path(manifest['kit']),1048576),
        settings=json.loads(_read_json_text(path(manifest['settings']),262144)),
        ladder_quality_definition=_read_json_text(path(manifest['ladder_quality']),1048576),
        missing_offset_policy=manifest['missing_offset_policy'])
    text=json.dumps(report,ensure_ascii=False,allow_nan=False,indent=2)+'\n'
    if args.output:
        with args.output.open('x',encoding='utf-8') as destination:destination.write(text)
        print(args.output.resolve())
    else:print(text,end='')
    return report


def _compact_project_report(report, explicit_status=None):
    if type(report) is not dict:
        raise ValueError("Analysis report must be a JSON object")
    source = report.get("source_sha256")
    version = report.get("analysis_version")
    if not isinstance(version, str) or not version.strip():
        raise ValueError("Analysis report requires analysis_version")
    settings = report.get("settings", report.get("peak_settings"))
    if type(settings) is not dict:
        raise ValueError("Analysis report requires settings or peak_settings object")
    sizing = report.get("sizing")
    report_status = report.get("status")
    if report_status is None and type(sizing) is dict:
        report_status = sizing.get("status")
    status = report_status if explicit_status is None else explicit_status
    if not isinstance(status, str) or not status.strip():
        raise ValueError("Report must state status, or supply --status explicitly")
    if status == "accepted" and report_status != "accepted":
        raise ValueError("accepted status must be explicitly present in the analysis JSON")
    if report_status is not None and explicit_status is not None and explicit_status != report_status:
        raise ValueError("--status must agree with the report's explicit status")
    result = {key: report[key] for key in _PROJECT_RESULT_FIELDS if key in report}
    result["status"] = status
    metadata = {}
    if isinstance(report.get("filename"), str):
        metadata["file_name"] = report["filename"]
    return source, {"name": "OpenTrace", "version": version}, settings, result, status, metadata


def _project_command(args):
    action = args.project_action
    if action == "create":
        with ProjectStore.create(args.database, args.name) as store:
            return {"database": str(store.path), "project": store.project}
    if action == "import":
        with ProjectStore.import_json(args.database, _read_json_text(args.input)) as store:
            return {"database": str(store.path), "project": store.project}
    with ProjectStore.open(args.database) as store:
        if action=='upgrade':
            with store.upgraded() as upgraded:
                with upgraded._save_new(args.output) as saved:
                    return {'database':str(saved.path),'project':saved.project}
        if action.startswith('run-'):
            from .project_runs import create_project_run,revise_project_run,change_run_ladder
            if action=='run-show':
                return {'run':store.get_run(args.run_id),'revisions':store.list_run_revisions(args.run_id)}
            if action=='run-create':
                return create_project_run(store,args.name,_read_json_text(args.inputs,16*1024*1024),actor=args.actor,reason=args.reason)
            if action=='run-revise':
                return revise_project_run(store,args.run_id,args.parent_revision,_read_json_text(args.inputs,16*1024*1024),
                    actor=args.actor,reason=args.reason)
            return change_run_ladder(store,args.run_id,args.parent_revision,args.ladder_id,
                analysis=json.loads(_read_json_text(args.analysis,16*1024*1024)) if action=='run-replace-ladder' else None,
                remove=action=='run-remove-ladder',actor=args.actor,reason=args.reason)
        if action in ("list", "show"):
            if action == "show" and args.analysis_id:
                return {"analysis": store.get_analysis(args.analysis_id),
                        "effective": store.effective_analysis(args.analysis_id),
                        "review_events": store.review_events(args.analysis_id)}
            return {"project": store.project, "samples": store.list_samples(),
                    "analyses": [{key: analysis[key] for key in
                                  ("analysis_id", "sample_id", "input_sha256", "status", "created_at",
                                   "algorithm_sha256", "settings_sha256", "result_sha256")}
                                 for analysis in store.list_analyses()]}
        if action == "export":
            document = store.export_json()
            with args.output.open("x", encoding="utf-8", newline="") as destination:
                destination.write(document + "\n")
            return {"output": str(args.output.resolve())}
        if action == "save":
            report = json.loads(_read_json_text(args.report))
            source, algorithm, settings, result, status, metadata = _compact_project_report(report, args.status)
            metadata["report_file_name"] = args.report.name
            sample_id = args.sample_id
            if sample_id is not None:
                matches = [sample for sample in store.list_samples() if sample["sample_id"] == sample_id]
                if not matches or matches[0]["source_sha256"] != source:
                    raise ValueError("--sample-id must identify a sample with the report source SHA-256")
            # Validate the complete candidate before adding a sample to the real
            # immutable project, so invalid reports leave no orphan sample record.
            with tempfile.TemporaryDirectory(prefix="opentrace-project-check-") as temporary:
                with ProjectStore.create(Path(temporary) / "candidate.sqlite", "Report validation") as candidate:
                    candidate_sample = candidate.add_sample(source, metadata)
                    candidate.add_analysis(candidate_sample, source, algorithm, settings, result, status)
            if sample_id is None:
                sample_id = store.add_sample(source, metadata)
            analysis_id = store.add_analysis(sample_id, source, algorithm, settings, result, status)
            return {"sample_id": sample_id, "analysis_id": analysis_id, "status": status}
        if action == "review":
            identifier = store.append_review(args.analysis_id, args.action, actor=args.actor, reason=args.reason,
                                             target_kind=args.target_kind, target_id=args.target_id, value=args.value)
        else:
            identifier = store.undo_review(args.analysis_id, args.event_id, actor=args.actor, reason=args.reason)
        return {"event_id": identifier, "effective": store.effective_analysis(args.analysis_id)}


def main(argv=None):
    parser = argparse.ArgumentParser(description="OpenTrace: independent research electropherogram tools")
    commands = parser.add_subparsers(dest="command", required=True)
    ui = commands.add_parser("serve", help="Start loopback-only browser UI")
    ui.add_argument("--port", type=int, default=8765)
    ui.add_argument("--open", action="store_true", help="Open browser")
    demo = commands.add_parser("demo", help="Write original synthetic six-channel FSA")
    demo.add_argument("output", type=Path)
    demo_kind = demo.add_mutually_exclusive_group()
    demo_kind.add_argument("--sizing", action="store_true", help="Include an original synthetic size-standard trace")
    demo_kind.add_argument("--pipeline", action="store_true", help="Write the slow-migration morphology/ratio sizing fixture")
    inspect = commands.add_parser("inspect", help="Print summary and optionally directory")
    inspect.add_argument("file", type=Path)
    inspect.add_argument("--tags", action="store_true")
    export = commands.add_parser("export", help="Export all numeric DATA traces or exploratory peaks")
    export.add_argument("file", type=Path)
    export.add_argument("--output", "-o", required=True, type=Path)
    export.add_argument("--peaks", action="store_true")
    export.add_argument("--threshold", type=float, default=100)
    export.add_argument("--distance", type=int, default=8)
    export.add_argument("--anchors", default="", help="Manual scan:bp pairs, e.g. 500:75,2000:400")
    sizing = commands.add_parser("size", help="Match an explicit size standard and output its research report")
    sizing.add_argument("file", type=Path)
    sizing.add_argument("--output", "-o", type=Path)
    sizing.add_argument("--threshold", type=float, default=100)
    sizing.add_argument("--distance", type=int, default=8)
    _project_parser(commands)
    str_command=commands.add_parser('str',help='Analyze an explicit sample/ladder pair using an explicit kit JSON')
    str_command.add_argument('sample',type=Path)
    str_command.add_argument('--ladder',required=True,type=Path)
    str_command.add_argument('--kit',required=True,type=Path)
    str_command.add_argument('--settings',required=True,type=Path,help='Explicit analyze() keyword settings as JSON')
    str_command.add_argument('--missing-offset-policy',choices=('reject','forward_fill'),default='reject')
    str_command.add_argument('--ladder-quality',type=Path,help='Explicit ladder GQ/SQ subset gate definition JSON')
    str_command.add_argument('--output','-o',type=Path)
    run_command=commands.add_parser('str-run',help='Analyze a sample with an explicit multi-ladder run manifest')
    run_command.add_argument('manifest',type=Path)
    run_command.add_argument('--output','-o',type=Path)
    run_demo=commands.add_parser('str-run-demo',help='Write an invented three-ladder run and its report')
    run_demo.add_argument('directory',type=Path)
    str_demo=commands.add_parser('str-demo',help='Create a new directory with two invented FSA files and their kit/settings/report')
    str_demo.add_argument('directory',type=Path)
    str_demo.add_argument('--variant',choices=('ordinary','negative','amel','ladder','ladder-rejected','spike','spike-unrequested','spike-low-initial','spike-low-adjusted','sq-pass','sq-check','sq-fail','sq-ladder-fail','ladder-calls','ladder-sample-filters','morphology','morphology-unrequested','true-peak','true-peak-excluded','pullup','pullup-flags-only','pullup-unrequested'),default='ordinary')
    controls=commands.add_parser('controls',help='Append explicit positive/negative control observations to a research STR report')
    controls.add_argument('report',type=Path)
    controls.add_argument('--definition',required=True,type=Path)
    controls.add_argument('--output','-o',type=Path)
    marker_quality=commands.add_parser('marker-quality',help='Annotate STR with explicit sample/negative GQ and optional AMEL cross-check')
    marker_quality.add_argument('report',type=Path)
    marker_quality.add_argument('--definition',required=True,type=Path)
    marker_quality.add_argument('--output','-o',type=Path)
    pullup_quality=commands.add_parser('pullup-quality',help='Annotate retained pull-up flags with marker triggers and profile category')
    pullup_quality.add_argument('report',type=Path)
    pullup_quality.add_argument('--definition',required=True,type=Path)
    pullup_quality.add_argument('--output','-o',type=Path)
    for command in (export, sizing):
        command.add_argument('--pullup-definition',type=Path,help='Explicit experimental pull-up routes and independent profile QC JSON')
        command.add_argument('--spike-definition',type=Path,help='Explicit fresh basic spike classifier JSON')
        command.add_argument('--sizing-quality-definition',type=Path,help='Explicit fresh post-sizing broad-width SQ JSON')
        command.add_argument('--analysis-engine', choices=('residual', 'morphology_ratio'), default='residual')
        command.add_argument('--height-mode', choices=('explicit', 'basic_standard_auto'), default='explicit')
        command.add_argument('--quality-threshold', type=float, default=0.75)
        command.add_argument('--num-extra-peaks', type=int, default=25)
        command.add_argument("--standard-channel", default="", help="Exact reference dye DATA tag, e.g. DATA106")
        command.add_argument("--standard-bp", default="", help="Expected fragment sizes separated by comma")
        command.add_argument("--max-residual-bp", type=float, default=5)
        command.add_argument("--standard-start", type=int, default=0)
        command.add_argument("--standard-end", type=int, default=None)
        command.add_argument("--calibration-model", default="linear",
                             choices=("linear", "lsq2", "lsq3", "natural_cubic", "local_rational"))
    args = parser.parse_args(argv)
    try:
        if args.command in ('controls','marker-quality','pullup-quality'):
            if args.command=='controls':
                from .control_workflow import attach_control_result as annotate
            elif args.command=='marker-quality':
                from .marker_quality_workflow import attach_marker_quality as annotate
            else:
                from .pullup_quality import attach_pullup_quality as annotate
            result=annotate(json.loads(_read_json_text(args.report)),_read_json_text(args.definition,1048576))
            output=json.dumps(result,ensure_ascii=False,allow_nan=False,indent=2)+'\n'
            if args.output:
                with args.output.open('x',encoding='utf-8') as destination:destination.write(output)
                print(args.output.resolve())
            else:print(output,end='')
        elif args.command in ('str-run','str-run-demo'):
            if _str_run_command(args)['status']=='rejected':return 3
        elif args.command in ('str','str-demo'):
            from .str_workflow import analyze_str_files
            if args.command=='str-demo':
                from .str_demo import (make_str_demo,make_quality_demo_definition,
                                       make_cross_check_demo,make_negative_demo_definitions,make_ladder_quality_demo_definition,make_spike_demo_definition,make_spike_adjustment_demo,make_post_sizing_demo,make_ladder_calling_demo,make_morphology_demo,make_true_peak_demo)
                controls=None
                ladder_quality_definition=(make_ladder_quality_demo_definition(reject=args.variant=='ladder-rejected')
                    if args.variant in ('ladder','ladder-rejected') else None)
                if args.variant=='amel':fixture,quality_definition=make_cross_check_demo()
                else:
                    fixture=make_str_demo(include_spike=args.variant in ('spike','spike-unrequested'));quality_definition=make_quality_demo_definition()
                    if args.variant=='negative':quality_definition,controls=make_negative_demo_definitions()
                    if args.variant=='spike':fixture['settings']['spike_definition']=make_spike_demo_definition()
                    if args.variant in ('spike-low-initial','spike-low-adjusted'):
                        fixture=make_spike_adjustment_demo(adjusted=args.variant=='spike-low-adjusted')
                    if args.variant in ('morphology','morphology-unrequested'):
                        fixture=make_morphology_demo(enabled=args.variant=='morphology')
                    if args.variant in ('true-peak','true-peak-excluded'):
                        fixture=make_true_peak_demo(exclude_overlap=args.variant=='true-peak-excluded')
                    if args.variant.startswith('pullup'):
                        from .str_demo import make_pullup_demo
                        fixture=make_pullup_demo(detector=args.variant=='pullup',requested=args.variant!='pullup-unrequested')
                    if args.variant.startswith('sq-'):
                        fixture=make_post_sizing_demo(args.variant);quality_definition['sample_sq']={'source':'post_sizing_quality'}
                        if args.variant=='sq-ladder-fail':
                            ladder_quality_definition=make_ladder_quality_demo_definition()
                            ladder_quality_definition['ladder_sq']={'source':'post_sizing_quality'}
                    if args.variant in ('ladder-calls','ladder-sample-filters'):
                        fixture,ladder_quality_definition=make_ladder_calling_demo(legacy=args.variant=='ladder-sample-filters')
                result=analyze_str_files(fixture['sample_bytes'],fixture['ladder_bytes'],fixture['kit'],settings=fixture['settings'],
                    ladder_quality_definition=ladder_quality_definition)
                if controls:
                    from .control_workflow import attach_control_result
                    result=attach_control_result(result,controls)
                if args.variant in ('negative','amel','spike','spike-unrequested','spike-low-initial','spike-low-adjusted','ladder-calls','ladder-sample-filters','morphology','morphology-unrequested','true-peak','true-peak-excluded','pullup','pullup-flags-only','pullup-unrequested') or args.variant.startswith('sq-'):
                    from .marker_quality_workflow import attach_marker_quality
                    result=attach_marker_quality(result,quality_definition)
                args.directory.mkdir(parents=False,exist_ok=False)
                for name,contents in (('sample.fsa',fixture['sample_bytes']),('ladder.fsa',fixture['ladder_bytes'])):
                    (args.directory/name).write_bytes(contents)
                documents=[('kit.json',fixture['kit']),('settings.json',fixture['settings']),('report.json',result),
                           ('marker-quality.json',quality_definition)]
                if controls:documents.append(('control.json',controls))
                if ladder_quality_definition:documents.append(('ladder-quality.json',ladder_quality_definition))
                if args.variant in ('spike','spike-unrequested'):documents.append(('spike.json',make_spike_demo_definition()))
                if args.variant in ('spike-low-initial','spike-low-adjusted','morphology','true-peak','true-peak-excluded'):
                    documents.append(('spike.json',fixture['settings']['spike_definition']))
                if args.variant.startswith('sq-'):
                    documents.append(('sizing-quality.json',fixture['settings']['sizing_quality_definition']))
                if 'pullup_definition' in fixture['settings']:
                    documents.append(('pullup.json',fixture['settings']['pullup_definition']))
                for name,contents in documents:
                    (args.directory/name).write_text(json.dumps(contents,ensure_ascii=False,allow_nan=False,indent=2)+'\n',encoding='utf-8')
                print(args.directory.resolve())
            else:
                settings=json.loads(_read_json_text(args.settings,262144))
                if not isinstance(settings,dict):
                    raise ValueError('Analysis settings must be an explicit JSON object')
                try:
                    result=analyze_str_files(args.sample.read_bytes(),args.ladder.read_bytes(),
                        _read_json_text(args.kit,1048576),settings=settings,missing_offset_policy=args.missing_offset_policy,
                        ladder_quality_definition=_read_json_text(args.ladder_quality,1048576) if args.ladder_quality else None)
                except TypeError as error:
                    raise ValueError('Invalid analysis settings: '+str(error)) from error
                output=json.dumps(result,ensure_ascii=False,allow_nan=False,indent=2)+'\n'
                if args.output:
                    with args.output.open('x',encoding='utf-8') as destination:destination.write(output)
                    print(args.output.resolve())
                else:print(output,end='')
            if result['status']=='rejected':return 3
        elif args.command == "project":
            print(json.dumps(_project_command(args), ensure_ascii=False, indent=2, allow_nan=False))
        elif args.command == "serve":
            serve(args.port, args.open)
        elif args.command == "demo":
            args.output.write_bytes(pipeline_demo_bytes() if args.pipeline else sizing_demo_bytes() if args.sizing else demo_bytes())
            print(args.output.resolve())
        elif args.command == "inspect":
            sample = AbifFile.read(args.file)
            result = sample.summary()
            if args.tags:
                result["directory"] = sample.directory()
            print(json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False))
        else:
            sample = AbifFile.read(args.file)
            automatic = bool(args.standard_channel or args.standard_bp or args.command == "size")
            if args.command == "export" and (automatic or args.calibration_model != "linear") and not args.peaks:
                raise ValueError("Use --peaks to export automatically sized peak candidates")
            if args.command == "size" or args.peaks:
                result = analyze(args.file.read_bytes(), args.file.name, args.threshold,
                                 args.distance, getattr(args, "anchors", ""),
                                 sizing_mode="auto" if automatic else "",
                                 standard_channel=args.standard_channel, standard_bp=args.standard_bp,
                                 max_residual_bp=args.max_residual_bp,
                                 standard_start=args.standard_start, standard_end=args.standard_end,
                                 calibration_model=args.calibration_model, analysis_engine=args.analysis_engine,
                                 height_mode=args.height_mode, quality_threshold=args.quality_threshold,
                                 num_extra_peaks=args.num_extra_peaks,
                                 spike_definition=_read_json_text(args.spike_definition,16384) if args.spike_definition else None,
                                 pullup_definition=_read_json_text(args.pullup_definition,16384) if args.pullup_definition else None,
                                 sizing_quality_definition=_read_json_text(args.sizing_quality_definition,16384) if args.sizing_quality_definition else None)
                if args.command == "size":
                    report = {key: result[key] for key in ("filename", "source_sha256", "analysis_version",
                              "peak_settings", "sizing", "research_only")}
                    output = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
                else:
                    output = peaks_csv(result["channels"])
            else:
                output = traces_csv(sample.channels())
            if args.output:
                args.output.write_text(output, encoding="utf-8", newline="")
                print(args.output.resolve())
            else:
                print(output, end="")
            if (args.command == "size" or args.peaks) and result["sizing"]["status"] == "rejected":
                print("Automatic sizing rejected: " + "; ".join(result["sizing"]["reasons"]), file=sys.stderr)
                return 3
        return 0
    except (AbifError, ValueError, OSError, sqlite3.DatabaseError) as exc:
        print(f"Error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
