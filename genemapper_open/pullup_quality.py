"""Marker bleedthrough triggers and independent categorical profile PQV.

Consumes flags from every linked window peak, including artifacts excluded from
calling. It never infers a ratio from signal, changes GQ/confidence, or admits
ladder offsets. Original automatic detector reachability is a separate issue.
"""
from collections.abc import Mapping
from copy import deepcopy
import hashlib
import json
from .pullup import load_pullup_definition,_flag
from .quality_control import _number,profile_pullup_category


def _digest(value):
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),
        ensure_ascii=False,allow_nan=False).encode()).hexdigest()


def evaluate_pullup_quality(report,definition):
    definition=load_pullup_definition(definition)
    if not isinstance(report,Mapping):raise ValueError('STR observations required')
    result={'schema_version':1,'status':'unavailable','reason':'calling_observations_unavailable',
        'definition_sha256':_digest(definition),'markers':[],'profile':None,
        'scope':'linked_peak_bleedthrough_and_profile_pullup_only','gq_or_confidence_changed':False,
        'ladder_admission_changed':False,'summary_pass_changed':False,
        'full_original_pqv':False,'original_runtime_validated':False}
    calling=report.get('allele_calling')
    if not isinstance(calling,Mapping):return result
    markers=calling.get('markers');peaks=report.get('peaks',[])
    if not isinstance(markers,list) or len(markers)>128 or not isinstance(peaks,list) or len(peaks)>4096:
        raise ValueError('Bounded marker and source peak lists required')
    if any(not isinstance(peak,Mapping) or not isinstance(peak.get('id'),str) for peak in peaks):
        raise ValueError('Source peaks with stable IDs required')
    if len({peak['id'] for peak in peaks})!=len(peaks):raise ValueError('Unique source peak IDs required')
    names=set();values=[]
    for marker in markers:
        if not isinstance(marker,Mapping) or not isinstance(marker.get('name'),str):raise ValueError('Named marker result required')
        name=marker['name']
        if name in names:raise ValueError('Unique marker results required')
        names.add(name)
        row={'marker':name,'status':'unavailable','metric':None,'triggers':[],'linked_peak_count':None,
             'reason':'marker_processing_unavailable'}
        if marker.get('status')=='tentative':
            linked=marker.get('peak_review')
            if not isinstance(linked,list) or len(linked)>512:raise ValueError('Complete bounded marker-window links required')
            seen=set()
            for entry in linked:
                if not isinstance(entry,Mapping):raise ValueError('Linked peak entry required')
                index=entry.get('peak_index')
                if type(index) is not int or not 0<=index<len(peaks) or index in seen:raise ValueError('Unique valid linked peak indices required')
                seen.add(index);peak=peaks[index]
                for key in ('scan','size_bp','height'):
                    if entry.get(key)!=peak.get(key):raise ValueError('Linked/source peak measurements disagree')
                if _flag(entry,'pullup')!=_flag(peak,'pullup'):raise ValueError('Linked/source pull-up flags disagree')
                if not _flag(entry,'pullup'):continue
                classification=peak.get('pullup_classification') or {}
                state=classification.get('state') or {}
                ratio=_number(state.get('ratio',peak.get('pullup_ratio',-1)),'retained pull-up ratio',-1e12,1e12)
                threshold=_number(state.get('ratio_threshold',peak.get('pullup_ratio_threshold',-1)),'retained pull-up threshold',-1,1e12)
                position=_number(peak.get('size_bp'),'pull-up trigger position',-1e6,1e6)
                row['triggers'].append({'id':'PTI_BLEED_THROUGH','ordinal':4,'source_peak_id':peak['id'],
                    'ratio':ratio,'thresholds':[threshold],'positions_bp':[position],
                    'ratio_provenance':classification.get('ratio_provenance','stored_flag_without_detector_measurement'),
                    'ratio_available':ratio>=0 and threshold>=0})
            row.update(status='evaluated',metric=int(bool(row['triggers'])),linked_peak_count=len(linked),reason=None)
        values.append(row['metric']);result['markers'].append(row)
    profile=profile_pullup_category(values,threshold=definition['profile_threshold'])
    result.update(profile=profile,status='evaluated_subset' if all(v is not None for v in values) else 'partial',reason=None,
        input_observations_sha256=_digest({'peaks':peaks,'allele_calling':calling}))
    return result


def attach_pullup_quality(report,definition):
    result=deepcopy(report)
    quality=evaluate_pullup_quality(report,definition)
    result.setdefault('quality',{})['pullup']=quality
    result.setdefault('settings',{})['pullup_quality_definition_sha256']=quality['definition_sha256']
    return result
