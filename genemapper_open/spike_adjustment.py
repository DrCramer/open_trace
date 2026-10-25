"""Independent current-spike transitions on fresh, explicit call observations.

Only the current boolean is reconstructed. Original artifact labels, OMR,
dirty-state and linked Java objects are outside this model. No calling stage
is repeated after a flag is cleared. Marker order is our declared kit order.
"""
from collections.abc import Mapping
from .fastcopy import fastcopy
from .quality_control import _number, _bool


def validate_adjustments(policy):
    keys={'sample_cutoff','sample_marker_envelope','ladder_height_ratio'}
    if not isinstance(policy,Mapping) or set(policy)!=keys:
        raise ValueError('Explicit sample_cutoff, sample_marker_envelope and ladder_height_ratio required')
    _bool(policy['sample_cutoff'],'sample_cutoff')
    _bool(policy['sample_marker_envelope'],'sample_marker_envelope')
    _number(policy['ladder_height_ratio'],'ladder_height_ratio',0,1e12)
    return fastcopy(dict(policy))


class SpikeState:
    """Internal state over bounded, normalized call_sample peak records."""
    def __init__(self,prepared,policy):
        self.policy=validate_adjustments(policy)
        self.peaks={p['peak_index']:p for p in prepared}
        self.states={i:{'peak_index':i,'initial_spike':'spike' in p['flags'],
                        'current_spike':'spike' in p['flags'],'events':[]}
                     for i,p in self.peaks.items()}

    def clear(self,index,reason,**details):
        state=self.states[index]
        if state['current_spike']:
            state['current_spike']=False
            state['events'].append({'reason':reason,**details})
            self.peaks[index]['flags'].remove('spike')

    def marker_metric(self,marker,phase,**details):
        indices=[p['peak_index'] for p in marker['peak_review']]
        remaining=[i for i in indices if self.states[i]['current_spike']]
        marker['spike_state']={'phase':phase,
            'entry_spike_indices':[p['peak_index'] for p in marker['peak_review'] if 'spike' in p['flags']],
            'remaining_peak_indices_at_metric':remaining,'metric':int(bool(remaining)),**details}

    def after_sample_marker(self,marker):
        cutoff=marker.get('cutoff_height')
        candidates=sum(not any(f in p['flags'] for f in ('spike','pullup')) for p in marker['peak_review'])
        # Missing cutoff means the candidate list was empty, not a zero-height
        # surrogate. The original's positive-cutoff gate is strict.
        if self.policy['sample_cutoff'] and candidates and cutoff is not None and cutoff>0:
            for review in marker['peak_review']:
                i=review['peak_index']
                if review['height']<=cutoff:
                    self.clear(i,'sample_height_at_or_below_cutoff',marker=marker['name'],cutoff_height=cutoff)
        self.marker_metric(marker,'after_marker_cutoff_before_channel_envelope',
                           cutoff_height=cutoff,candidate_count=candidates)

    def finish_sample(self,markers):
        envelopes={}
        for marker in markers:
            if marker['status']!='tentative':continue
            low,high=marker['window_bp'];channel=marker['channel']
            if channel in envelopes:
                low=min(low,envelopes[channel][0]);high=max(high,envelopes[channel][1])
            envelopes[channel]=[low,high]
        if self.policy['sample_marker_envelope']:
            for i,peak in self.peaks.items():
                bounds=envelopes.get(peak['channel'])
                if bounds and not bounds[0]<=peak['size_bp']<=bounds[1]:
                    self.clear(i,'outside_channel_marker_envelope',window_bp=bounds)
        # Keep each marker metric at its observed phase. Do not recompute it
        # after a later overlapping marker or the channel envelope changes flags.
        return self.summary('sample',channel_envelopes=envelopes)

    def summary(self,kind,**details):
        states=[fastcopy(self.states[i]) for i in sorted(self.states)]
        return {'status':'evaluated_subset','sample_kind':kind,'policy':fastcopy(self.policy),
            'initial_spikes':sum(s['initial_spike'] for s in states),
            'current_spikes':sum(s['current_spike'] for s in states),
            'cleared_spikes':sum(s['initial_spike'] and not s['current_spike'] for s in states),
            'peak_states':states,'calling_repeated':False,'marker_order':'kit_declared_order',
            'scope':'current_spike_boolean_on_sized_kit_channels',
            'external_validation':False,'original_runtime_validated':False,**details}


def adjust_ladder_spikes(peaks,observations,policy):
    """Clear after all ladder calls, per dye; preserve the original exclusion.

    The original caller passes results of one dye to the maximum-height helper.
    We therefore use the maximum across processed markers of the SAME channel,
    never a different channel and never an excluded candidate or stutter call.
    """
    from .allele_calling import _prepare_peaks
    state=SpikeState(_prepare_peaks(peaks),policy)
    observed=fastcopy(observations)
    by_channel={}
    for marker in observed['markers']:
        if marker['status']=='tentative':by_channel.setdefault(marker['channel'],[]).append(marker)
    thresholds={}
    for channel,markers in by_channel.items():
        maximum=max((c['height'] for m in markers for c in m['calls']),default=None)
        ratio=state.policy['ladder_height_ratio']
        cutoff=maximum*ratio if maximum is not None and maximum>0 and ratio>0 else None
        thresholds[channel]={'max_called_height':maximum,'cutoff_height':cutoff}
        if cutoff is not None:
            for i,peak in state.peaks.items():
                if peak['channel']==channel and peak['height']<=cutoff:
                    state.clear(i,'ladder_height_at_or_below_cutoff',cutoff_height=cutoff)
        for marker in markers:
            state.marker_metric(marker,'after_ladder_height_cutoff',cutoff_height=cutoff)
    observed['spike_adjustments']=state.summary('ladder',channel_thresholds=thresholds)
    return observed


def apply_peak_states(peaks,summary):
    """Copy scalar current states onto our own source-peak model, retaining history."""
    result=fastcopy(peaks)
    for state in summary['peak_states']:
        peak=result[state['peak_index']]
        peak['spike_adjustment']=fastcopy(state)
        peak['spike']=state['current_spike']
        flags=peak.get('flags')
        if isinstance(flags,Mapping):
            flags['spike']=state['current_spike']
        elif isinstance(flags,(list,tuple)):
            peak['flags']=[f for f in flags if f!='spike']+(['spike'] if state['current_spike'] else [])
    return result


def marker_has_spike(marker):
    """Read a phase-specific metric; older initial-only reports retain their path."""
    state=marker.get('spike_state')
    if state is not None:
        if not isinstance(state,Mapping) or type(state.get('metric')) is not int or state['metric'] not in (0,1):
            raise ValueError('Explicit binary phase-specific spike metric required')
        return state['metric']==1
    return any('spike' in p.get('flags',[]) for p in marker.get('peak_review',[]))
