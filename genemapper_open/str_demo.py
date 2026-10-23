"""Invented end-to-end STR mathematics fixture; no commercial kit constants."""
import math
import struct
from .demo import build_abif, SYNTHETIC_PIPELINE_STANDARD_BP


def make_str_demo(*,include_spike=False,spike_height=1100,ladder_heights=None,ladder_shift_bp=0,jag_height=0,pullup_reference_height=0):
    kit = {'schema_version':1, 'name':'Invented STR workflow example', 'markers':[{
        'name':'ExampleM', 'channel':'DATA1', 'min_bp':160, 'max_bp':200, 'repeat':4,
        'bins':[{'label':label, 'center_bp':bp, 'left_bp':0.5, 'right_bp':0.5}
                for label,bp in zip('ABCD',(168,176,184,192))],
        'ladder_labels':list('ABCD'), 'parameters':{
            'ladder_cutoff_ratio':0.05, 'cutoff_ratio':0.1, 'minus_a_ratio':0.1,
            'minus_a_min_bp':0.8, 'minus_a_max_bp':1.2,
            'backward_stutter_ranges':[{'min_bp':3.8,'max_bp':4.2,'ratio':0.15}],
            'forward_stutter_ranges':[], 'clear_ranges':[]}}]}
    def data(ladder):
        name = b'Invented ladder' if ladder else b'Invented sample'
        items = [(b'SMPL',1,18,1,len(name)+1,bytes([len(name)])+name),
                 (b'Dye#',1,4,2,1,struct.pack('>h',6))]
        for index,number in enumerate((1,2,3,4,105,106),1):
            dye = f'Example dye {index}'.encode('ascii')
            items.append((b'DyeN',index,18,1,len(dye)+1,bytes([len(dye)])+dye))
            bp = SYNTHETIC_PIPELINE_STANDARD_BP if number == 106 else (
                tuple(x+ladder_shift_bp for x in (168.3,176.3,184.3,192.3)) if ladder else (176.3,184.3)) if number == 1 else ()
            if number==2 and not ladder and pullup_reference_height>0:bp=(176.3,)
            values = [round(20+0.001*scan+sum((pullup_reference_height if number==2 else ladder_heights[i] if ladder and number==1 and ladder_heights is not None else 1000)*math.exp(-.5*((scan-(200+20*size))/5)**2)
                      for i,size in enumerate(bp))+(spike_height*math.exp(-.5*((scan-(200+20*168.3))/.65)**2)
                      if include_spike and not ladder and number==1 else 0)+
                      (jag_height if not ladder and number==1 and scan==3726 else 0)) for scan in range(6501)]
            items.append((b'DATA',number,4,2,len(values),struct.pack(f'>{len(values)}h',*values)))
        return build_abif(items)
    return {'sample_bytes':data(False), 'ladder_bytes':data(True), 'kit':kit,
            'settings':{'sizing_mode':'auto', 'analysis_engine':'morphology_ratio',
                'standard_channel':'DATA106', 'standard_bp':list(SYNTHETIC_PIPELINE_STANDARD_BP),
                'threshold':100, 'calibration_model':'local_rational', 'quality_threshold':0.75,
                'height_mode':'explicit', 'num_extra_peaks':25}}


def make_quality_demo_definition():
    """Explicit mathematical settings for the invented ExampleM fixture only."""
    from .marker_quality import WEIGHTS
    return {'schema_version':1,'sample_sq':{'source':'base_quality'},'gq_thresholds':{'good':.75,'bad':.25},
        'amel_marker':None,'control_mode':'none','markers':{'ExampleM':{
            'kind':'autosomal','confidence_source':'bin_confidence_weight',
            'thresholds':{'max_alleles':2,'homozygous_height':100,'heterozygous_height':50,
                          'high_height':5000,'ratio':.5,'width_bp':2},
            'weights':dict.fromkeys(WEIGHTS,.5),
            'flags':dict.fromkeys(('spike','offscale','overlap','binning'),False)}}}


def make_run_demo():
    """Two admitted shifts and a third failed-height ladder, all invented."""
    fixture=make_str_demo()
    ladders=[{'ladder_id':name,'data':make_str_demo(ladder_shift_bp=shift,ladder_heights=heights)['ladder_bytes']}
        for name,shift,heights in (('Own-A.fsa',-.2,None),('Own-B.fsa',.2,None),
            ('Own-Failed.fsa',3,[400,1000,1000,1000]))]
    return {**fixture,'ladders':ladders,'ladder_quality_definition':make_ladder_quality_demo_definition(),
        'marker_quality_definition':make_quality_demo_definition()}


def make_cross_check_demo():
    """Reuse invented FSA bytes with two explicit artificial marker windows."""
    from copy import deepcopy
    fixture=make_str_demo()
    prototype=fixture['kit']['markers'][0]
    amel=deepcopy(prototype);y=deepcopy(prototype)
    amel.update(name='AMEL',min_bp=160,max_bp=180,bins=amel['bins'][:2],ladder_labels=['Y','X'])
    for entry,label in zip(amel['bins'],('Y','X')):entry['label']=label
    y.update(name='ExampleY',min_bp=180,max_bp=200,bins=y['bins'][2:],ladder_labels=['1','2'])
    for entry,label in zip(y['bins'],('1','2')):entry['label']=label
    fixture['kit'].update(name='Invented AMEL cross-check example',markers=[amel,y])
    definition=make_quality_demo_definition();recipe=definition['markers'].pop('ExampleM')
    definition.update(schema_version=2,sample_type='Sample',amel_marker='AMEL',amel_cross_check={'weight':.5})
    definition['markers']={'AMEL':deepcopy(recipe),'ExampleY':deepcopy(recipe)}
    definition['markers']['AMEL']['kind']='amel';definition['markers']['ExampleY']['kind']='y'
    return fixture,definition


def make_negative_demo_definitions():
    """A nonblank invented negative control: GQ0 retains call confidence1."""
    quality=make_quality_demo_definition()
    quality.update(schema_version=2,sample_type='Negative Control',control_mode='negative_attached',amel_cross_check=None)
    quality['markers']={'ExampleM':{'kind':'autosomal','confidence_source':'bin_confidence_weight',
                                   'spike_metric':0,'spike_weight':.5}}
    control={'schema_version':1,'sample_type':'Negative Control','markers':{
        'ExampleM':{'expected_alleles':[],'quality_sensor':False,'is_y_marker':False}}}
    return quality,control


def make_ladder_quality_demo_definition(*,reject=False):
    """Explicit invented ladder QC, with an optional impossible balance gate."""
    return {'schema_version':1,'calling_parameters_source':'ladder_matching_calls',
        'ladder_sq':{'source':'base_quality'},'sq_thresholds':{'good':.75,'bad':.25},
        'gq_thresholds':{'good':.75,'bad':.25},'markers':{'ExampleM':{
            'min_height_ratio':1 if reject else .2,'spike_metric':0,'spike_weight':.5,
            'offscale_metric':0,'offscale_weight':.5,'spacing':{'enabled':True,'max_distance_bp':20,
                'curve':[{'distance_bp':0,'fraction':.2,'interpolate':False}],
                'exceptions':[],'excluded_alleles':[]}}}}


def make_spike_demo_definition():
    """Explicit mathematical parameters for our normal/narrow generated peaks."""
    return {'schema_version':1,'measurement_source':'prepared_signal','scope':'non_reference_selected_channels',
        'morphology_enabled':False,'units':'scan','min_height':100,'exhalf_width_threshold':6,
        'near_base_width_threshold':10,'area_ratio_threshold':.8,'treat_overlap_as_spike':True,
        'near_base_fraction':.1,'max_base_height':-1}


def make_spike_adjustment_demo(*,adjusted=True):
    """Same own low-spike bytes with initial-only or explicitly adjusted policy."""
    fixture=make_str_demo(include_spike=True,spike_height=300)
    fixture['kit']['markers'][0]['parameters']['cutoff_ratio']=.2
    definition=make_spike_demo_definition()
    if adjusted:
        definition.update(schema_version=2,adjustments={'sample_cutoff':True,
            'sample_marker_envelope':True,'ladder_height_ratio':.2})
    fixture['settings']['spike_definition']=definition
    return fixture


def make_morphology_demo(*,enabled=True,pullup_reference_height=0):
    """Identical invented FSA with an isolated sharp component on the broad B peak."""
    fixture=make_str_demo(jag_height=600,pullup_reference_height=pullup_reference_height)
    if enabled:
        definition=make_spike_demo_definition()
        definition.update(schema_version=3,measurement_source='baseline_corrected_companion',morphology_enabled=True,
            morphology={'jag_threshold':.2,'fit_score_threshold':.995},
            adjustments={'sample_cutoff':False,'sample_marker_envelope':False,'ladder_height_ratio':0})
        fixture['settings']['spike_definition']=definition
    return fixture


def make_true_peak_demo(*,exclude_overlap=False,pullup_reference_height=0):
    """Same own broad-plus-sharp FSA; explicit full fitting and overlap policy."""
    from .small_svd import SOLVER_ID
    fixture=make_morphology_demo(pullup_reference_height=pullup_reference_height)
    fixture['settings']['spike_definition'].update(schema_version=4,treat_overlap_as_spike=exclude_overlap,
        true_peak={'polynomial_solver':SOLVER_ID,'max_denominator_scan2':10000})
    return fixture


def make_pullup_demo_definition(*,detector=True):
    """Own explicitly ordered route; private original body reachability unknown."""
    return {'schema_version':1,'profile_threshold':.4,'detector':{
        'algorithm':'static_ar_pairwise_body','calling_policy':'explicit_ordered_channel_pairs',
        'phase':'after_spike_restoration_before_calling','reference_order':'stable_scan_midpoint_binary_search',
        'max_scan_distance':1,'height_ratio_threshold':.08,
        'channel_pairs':[{'candidate_channel':'DATA1','reference_channel':'DATA2'}]} if detector else None}


def make_pullup_demo(*,detector=True,requested=True):
    fixture=make_str_demo(pullup_reference_height=18000)
    if requested:fixture['settings']['pullup_definition']=make_pullup_demo_definition(detector=detector)
    return fixture


def make_post_sizing_demo(kind='sq-check'):
    """Same own FSA bytes, explicit width penalty/score-source demonstration."""
    if kind not in ('sq-pass','sq-check','sq-fail','sq-ladder-fail'):raise ValueError('Unknown own SQ fixture')
    fixture=make_str_demo()
    fixture['settings']['sizing_quality_definition']={'schema_version':1,'context':'fresh_hid_no_override',
        'width_conversion':'unrounded_standard_map_span','warning_width_bp':2 if kind=='sq-pass' else .1,
        'broad_weight':.8 if kind in ('sq-fail','sq-ladder-fail') else .5,'thresholds':{'good':.75,'bad':.25}}
    return fixture


def make_ladder_calling_demo(*,legacy=False):
    """Unequal own ladder heights expose accidental reuse of sample minus-A."""
    fixture=make_str_demo(ladder_heights=[700,900,1100,1300])
    fixture['kit']['markers'][0]['parameters'].update(minus_a_ratio=.95,minus_a_min_bp=7.5,minus_a_max_bp=8.5)
    quality=make_ladder_quality_demo_definition()
    if legacy:quality['calling_parameters_source']='kit_sample_parameters'
    return fixture,quality
