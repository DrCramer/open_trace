"""Stateless browser project actions, validated in SQLite memory only."""
from .project_store import ProjectStore
from datetime import datetime


def project_action(request):
    if not isinstance(request,dict):
        raise ValueError('Project request must be an object')
    action=request.get('action')
    run_actions=('run-create','run-revise','run-replace-ladder','run-remove-ladder','run-select')
    if action not in ('new','import','save','select','review','undo',*run_actions):
        raise ValueError('Unknown project action')
    store=(ProjectStore.memory(request.get('name')) if action=='new'
           else ProjectStore.from_json(request.get('document')))
    if action in run_actions and store.project['schema_version']==1:
        upgraded=store.upgraded();store.close();store=upgraded
    with store:
        selected=None if action in ('new','import') else request.get('analysis_id')
        selected_run=request.get('run_id');selected_revision=request.get('revision_id');change=None
        if action=='save':
            report=request.get('report')
            if not isinstance(report,dict) or report.get('schema_version')!=1:
                raise ValueError('A version1 compact STR workflow report is required')
            if report.get('status') not in ('tentative','partial','rejected'):
                raise ValueError('STR workflow status must remain tentative, partial or rejected')
            fields=('schema_version','source_sha256','analysis_version','status','reasons','calls','peaks',
                    'kit_identity','sample_identity','ladder_identity','sizing','ladder','allele_calling','quality','provenance','research_only')
            result={key:report[key] for key in fields if key in report}
            source=report.get('source_sha256')
            samples=store.list_samples()
            identifier=next((s['sample_id'] for s in samples if s['source_sha256']==source),None)
            if identifier is None:
                identifier=store.add_sample(source,{'kind':'research STR sample'})
            selected=store.add_analysis(identifier,source,{'name':'OpenTrace STR','version':report.get('analysis_version')},
                                        report.get('settings'),result,report['status'])
        elif action=='review':
            store.append_review(selected,request.get('review_action'),actor=request.get('actor'),
                reason=request.get('reason'),target_kind=request.get('target_kind','analysis'),
                target_id=request.get('target_id'),value=request.get('value'))
        elif action=='undo':
            store.undo_review(selected,request.get('event_id'),actor=request.get('actor'),reason=request.get('reason'))
        elif action in run_actions:
            from .project_runs import create_project_run,revise_project_run,change_run_ladder
            if action=='run-create':
                change=create_project_run(store,request.get('name'),request.get('inputs'),
                    actor=request.get('actor'),reason=request.get('reason'))
            elif action=='run-revise':
                change=revise_project_run(store,selected_run,request.get('parent_revision_id'),request.get('inputs'),
                    actor=request.get('actor'),reason=request.get('reason'))
            elif action in ('run-replace-ladder','run-remove-ladder'):
                change=change_run_ladder(store,selected_run,request.get('parent_revision_id'),request.get('ladder_id'),
                    analysis=request.get('analysis'),remove=action=='run-remove-ladder',
                    actor=request.get('actor'),reason=request.get('reason'))
            if change:
                selected_run=change['run_id'];selected_revision=change['revision_id']
                selected=change['outputs'][0]['analysis_id'] if change['outputs'] else None
        runs=store.list_runs()
        if selected_run is None and runs:selected_run=runs[0]['run_id']
        revisions=store.list_run_revisions(selected_run) if selected_run else []
        revision=(store.get_run_revision(selected_revision) if selected_revision else revisions[-1] if revisions else None)
        if revision and revision['run_id']!=selected_run:raise ValueError('Selected revision belongs to another run')
        if action=='run-select':selected=revision['outputs'][0]['analysis_id'] if revision and revision['outputs'] else None
        analyses=sorted(store.list_analyses(),key=lambda row:(datetime.fromisoformat(
            row['created_at'].replace('Z','+00:00')),row['analysis_id']),reverse=True)
        if selected is None and analyses:
            selected=analyses[0]['analysis_id']
        return {'document':store.export_json(),'project':store.project,
            'analyses':[{key:a[key] for key in ('analysis_id','sample_id','status','created_at','result_sha256')}
                        for a in analyses], 'selected_analysis_id':selected,
            'effective':store.effective_analysis(selected) if selected else None,
            'events':store.review_events(selected) if selected else [],
            'runs':runs,'selected_run_id':selected_run,'run_revisions':revisions,
            'selected_run_revision':revision,'run_change':change}
