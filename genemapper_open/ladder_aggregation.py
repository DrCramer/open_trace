"""Explicit per-marker multiple-ladder means, with original numeric gate codes.

Code meanings are supplied by the caller; this module does not manufacture
original AL/PQV results. Independent structured keys avoid original string
concatenation collisions. No arithmetic or missing-value fallback is hidden.
"""
from collections.abc import Mapping
import math
from .kit_definition import load_kit_definition


ALLOWED_CODES=(0.0,-2.0,0.5)


def aggregate_ladder_offsets(entries,kit):
    definition=load_kit_definition(kit)
    if not isinstance(entries,(list,tuple)) or not 1<=len(entries)<=128:
        raise ValueError('Explicit list of1..128 ladder entries required')
    ids=set();validated=[]
    for entry in entries:
        if not isinstance(entry,Mapping) or set(entry)!= {'ladder_id','quality_code','result'}:
            raise ValueError('ladder_id/quality_code/result required')
        identifier=entry['ladder_id'];code=entry['quality_code']
        if not isinstance(identifier,str) or not 1<=len(identifier)<=256 or identifier in ids:
            raise ValueError('Bounded unique ladder_id required')
        if type(code) not in (int,float) or not math.isfinite(code):
            raise ValueError('Explicit finite quality_code required')
        if not isinstance(entry['result'],Mapping) or not isinstance(entry['result'].get('markers'),Mapping):
            raise ValueError('Explicit ladder result with markers required')
        ids.add(identifier);validated.append(entry)
    output={'status':'rejected','reasons':[],'markers':{},'diagnostics':{
        'method':'unweighted_per_bin_mean_with_explicit_numeric_quality_gate',
        'admitted_quality_codes':list(ALLOWED_CODES),'source_contract':'IDX16_ALLELE_CALLING AL-05',
        'quality_codes_computed_here':False,'external_validation':False,
        'missing_offset_policy':'withhold_marker_if_no_admitted_complete_ladder'}}
    for marker in definition['markers']:
        accepted=[];excluded=[]
        for entry in validated:
            record=entry['result']['markers'].get(marker['name'])
            if entry['quality_code'] not in ALLOWED_CODES:
                excluded.append({'ladder_id':entry['ladder_id'],'reason':'quality_code_not_admitted'});continue
            if entry['result'].get('status') not in ('accepted','partial') or not isinstance(record,Mapping) or record.get('status')!='accepted':
                excluded.append({'ladder_id':entry['ladder_id'],'reason':'marker_offsets_not_accepted'});continue
            offsets=record.get('bin_offsets')
            needed=marker['ladder_labels']
            if not isinstance(offsets,Mapping) or not needed or any(label not in offsets for label in needed):
                raise ValueError('Accepted ladder needs all explicit ladder-label offsets')
            values=[offsets[label] for label in needed]+[record.get('min_offset_bp'),record.get('max_offset_bp')]
            if any(type(value) not in (int,float) or not math.isfinite(value) or abs(value)>1_000_000 for value in values):
                raise ValueError('Finite bounded offsets required')
            accepted.append((entry['ladder_id'],record))
        record={'status':'rejected','bin_offsets':{},'min_offset_bp':None,'max_offset_bp':None,
                'source_ladders':[identifier for identifier,_ in accepted],'excluded':excluded}
        if accepted:
            mean=lambda values:math.fsum(values)/len(accepted)
            record.update(status='accepted',bin_offsets={label:mean([r['bin_offsets'][label] for _,r in accepted])
                for label in marker['ladder_labels']},min_offset_bp=mean([r['min_offset_bp'] for _,r in accepted]),
                max_offset_bp=mean([r['max_offset_bp'] for _,r in accepted]))
        output['markers'][marker['name']]=record
    statuses=[record['status'] for record in output['markers'].values()]
    output['status']='accepted' if all(status=='accepted' for status in statuses) else 'partial' if 'accepted' in statuses else 'rejected'
    if output['status']!='accepted':output['reasons'].append('Some markers have no admitted complete ladder offsets')
    return output
