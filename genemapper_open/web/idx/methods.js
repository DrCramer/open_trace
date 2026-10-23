// Analysis methods: store, Analysis Method Editor and the OpenTrace Manager.
(function(){
const KEY='opentrace.methods.v1';
const DYES=['blue','green','yellow','red','purple','orange'];
const REPEATS=[['3','tri'],['4','tetra'],['5','penta'],['6','hexa']];
const WEIGHTS=[['bd','wBd'],['bin','wBin'],['ovl','wOvl'],['spk','wSpk'],['acc','wAcc'],['cc','wCc'],
 ['an','wAn'],['lph','wLph'],['mph','wMph'],['os','wOs'],['phr','wPhr']];
let post=null;
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);

function load(){try{const v=JSON.parse(localStorage.getItem(KEY)||'null');if(v&&Array.isArray(v.methods))return v}catch(e){}return {schema:1,methods:[]}}
function store(db){try{localStorage.setItem(KEY,JSON.stringify(db));return true}catch(e){return false}}
function list(){return load().methods}
function get(name){return list().find(m=>m.method.name===name)?.method||null}
function put(method,previousName){const db=load();const now=new Date().toISOString();
 const i=db.methods.findIndex(m=>m.method.name===(previousName??method.name));
 const row={method:clone(method),saved:now};if(i>=0)db.methods[i]=row;else db.methods.push(row);
 if(!store(db))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-methods'))}
function remove(name){const db=load();db.methods=db.methods.filter(m=>m.method.name!==name);store(db);window.dispatchEvent(new Event('opentrace-methods'))}
async function ensureDefault(){if(list().length)return;const r=await post('/api/analysis-method',{action:'factory'});put(r.method)}
async function factory(){return (await post('/api/analysis-method',{action:'factory'})).method}

// ---------- editor ----------
function numInput(obj,key,attrs={}){const i=el('input',{type:'text',class:'num',value:String(obj[key]),...attrs});
 i.addEventListener('change',()=>{const v=Number(i.value.replace(',','.'));if(i.value.trim()===''||!isFinite(v)){message(T('amInvalid'));i.value=String(obj[key]);return}
  obj[key]=attrs.int?Math.round(v):v});return i}
function check(obj,key,label){const c=el('input',{type:'checkbox'});c.checked=!!obj[key];c.addEventListener('change',()=>{obj[key]=c.checked});
 return el('label',{class:'chk'},c,document.createTextNode(' '+label))}
function radio(name,obj,key,value,label,onChange){const r=el('input',{type:'radio',name});r.checked=obj[key]===value;
 r.addEventListener('change',()=>{if(r.checked){obj[key]=value;onChange&&onChange()}});return el('label',{class:'rad'},r,document.createTextNode(' '+label))}
const row=(label,...kids)=>el('div',{class:'field'},el('label',{text:label}),el('span',{class:'inl'},...kids));
const box=(legend,...kids)=>el('fieldset',{},el('legend',{text:legend}),...kids);

function tabGeneral(m){return [row(T('amName'),(()=>{const i=el('input',{type:'text',id:'amName',value:m.name});i.addEventListener('input',()=>{m.name=i.value});return i})()),
 row(T('amAnalysisType'),el('span',{text:'HID'})),
 row(T('amDescription'),(()=>{const i=el('textarea',{rows:'3',cols:'40'});i.value=m.description;i.addEventListener('input',()=>{m.description=i.value});return i})()),
 row(T('amInstrument'),(()=>{const i=el('input',{type:'text',value:m.instrument});i.addEventListener('input',()=>{m.instrument=i.value});return i})())]}
function binSetNames(){try{return [...new Set(KitStore.load().kits.flatMap(k=>k.binSets.map(s=>s.name)))]}catch(e){return []}}
function tabAllele(m,redraw){const a=m.allele;
 const bs=el('select',{id:'amBinSet'});bs.append(new Option(T('noPanelChoice'),''));
 for(const n of binSetNames())bs.append(new Option(n,n));if(a.bin_set&&![...bs.options].some(o=>o.value===a.bin_set))bs.append(new Option(a.bin_set,a.bin_set));
 bs.value=a.bin_set||'';bs.addEventListener('change',()=>{a.bin_set=bs.value||null});
 const t=el('table',{class:'amgrid'});const head=el('tr',{},el('th',{text:T('amRepeatType')}));
 for(const [,k] of REPEATS)head.append(el('th',{text:T('rep_'+k)}));t.append(head);
 const line=(label,fields)=>{const tr=el('tr',{},el('td',{text:label}));
  for(const [r] of REPEATS){const td=el('td');fields.forEach((f,i)=>{if(i)td.append(document.createTextNode(' – '));td.append(numInput(a.repeats[r],f))});tr.append(td)}t.append(tr)};
 line(T('amCutoff'),['cutoff']);line(T('amMinusARatio'),['minus_a_ratio']);line(T('amMinusADistance')+' ('+T('amFrom')+' – '+T('amTo')+')',['minus_a_from','minus_a_to']);
 line(T('amGlobalMinusRatio'),['minus_stutter_ratio']);line(T('amGlobalMinusDist')+' ('+T('amFrom')+' – '+T('amTo')+')',['minus_stutter_from','minus_stutter_to']);
 line(T('amGlobalPlusRatio'),['plus_stutter_ratio']);line(T('amGlobalPlusDist')+' ('+T('amFrom')+' – '+T('amTo')+')',['plus_stutter_from','plus_stutter_to']);
 const rf=el('button',{class:'btn',type:'button',text:T('amRangeFilter')});rf.addEventListener('click',()=>rangeFilter(a));
 return [row(T('amBinSet'),bs),check(a,'use_marker_specific_stutter',T('amUseMarkerStutter')),t,
  row(T('amAmelCutoff'),numInput(a,'amelogenin_cutoff')),el('div',{class:'btnrow'},rf)]}
function rangeFilter(a){const work=clone(a.range_filter);
 dialog(T('rangeFilterTitle'),b=>{const t=el('table',{class:'amgrid'});t.append(el('tr',{},el('th'),el('th',{text:T('amFrom')+' (bp)'}),el('th',{text:T('amTo')+' (bp)'})));
  for(const d of DYES)t.append(el('tr',{},el('td',{text:T('dye_'+d)}),el('td',{},numInput(work[d],'from')),el('td',{},numInput(work[d],'to'))));b.append(t)},
  [[T('ok'),()=>{for(const d of DYES)if(work[d].from>work[d].to){message(T('fromLtTo'));return false}a.range_filter=work},true],[T('cancel'),()=>true]])}
function tabPeakDetector(m,redraw){const p=m.peak_detector;
 const ar=p.analysis_range,sr=p.sizing_range;
 const arStart=numInput(ar,'start',{int:true}),arStop=numInput(ar,'stop',{int:true}),srStart=numInput(sr,'start_bp'),srStop=numInput(sr,'stop_bp');
 const sync=()=>{arStart.disabled=arStop.disabled=ar.full;srStart.disabled=srStop.disabled=sr.all};sync();
 const sizing=el('select',{id:'amSizeCalling'});for(const [v,k] of [['local_southern','amLocalSouthern'],['lsq2','am2nd'],['lsq3','am3rd'],['cubic_spline','amCubic'],['global_southern','amGlobalSouthern']])sizing.append(new Option(T(k),v));
 sizing.value=p.size_calling;sizing.addEventListener('change',()=>{p.size_calling=sizing.value});
 const th=el('div',{class:'dyerow'});for(const d of DYES){th.append(el('span',{class:'dyelbl',style:`color:${{blue:'#1f4fd6',green:'#1a9b2e',yellow:'#9a8400',red:'#d32020',purple:'#7a35b5',orange:'#d0700a'}[d]}`,text:T('dyeLetter_'+d)}),numInput(p.thresholds,d,{int:true,id:'thr_'+d}))}
 return [el('div',{class:'cols'},
   el('div',{},box(T('amAnalysisRange'),el('div',{},radio('amAr',ar,'full',true,T('amFullRange'),sync),radio('amAr',ar,'full',false,T('amPartialRange'),sync)),row(T('amStartPt'),arStart),row(T('amStopPt'),arStop)),
    box(T('amSizingRange'),el('div',{},radio('amSr',sr,'all',true,T('amAllSizes'),sync),radio('amSr',sr,'all',false,T('amPartialSizes'),sync)),row(T('amStartSize'),srStart),row(T('amStopSize'),srStop)),
    box(T('amSmoothing'),el('div',{},radio('amSm',p,'smoothing','none',T('amNone')),radio('amSm',p,'smoothing','light',T('amLight')),radio('amSm',p,'smoothing','heavy',T('amHeavy'))),
     row(T('amBaselineWindow'),numInput(p,'baseline_window',{int:true}),el('span',{text:' '+T('amPts')}))),
    box(T('amSizeCalling'),sizing)),
   el('div',{},box(T('amPeakDetection'),el('div',{text:T('amAmpThresholds')}),th,
     row(T('amMinHalfWidth'),numInput(p,'min_half_width',{int:true}),el('span',{text:' '+T('amPts')})),
     row(T('amPolyDegree'),numInput(p,'polynomial_degree',{int:true})),row(T('amWindowSize'),numInput(p,'peak_window',{int:true}),el('span',{text:' '+T('amPts')})),
     el('div',{text:T('amSlope')}),row(T('amPeakStart'),numInput(p,'slope_start')),row(T('amPeakEnd'),numInput(p,'slope_end'))),
    box(T('amNormalization'),check(p,'normalization',T('amUseNormalization')))))]}
function tabPeakQuality(m){const q=m.peak_quality;
 const sp=q.ladder_spike;
 return [box(T('amLph'),row(T('amHomo'),numInput(q,'lph_homozygous')),row(T('amHetero'),numInput(q,'lph_heterozygous')),row(T('amMaxPeak'),numInput(q,'mph'))),
  box(T('amPhr'),row(T('amMinPhr'),numInput(q,'phr'))),box(T('amBd'),row(T('amMaxWidth'),numInput(q,'bd_bp'))),
  box(T('amAn'),el('div',{text:T('amMaxAlleles')}),row(T('amForAuto'),numInput(q,'max_alleles',{int:true})),row(T('amForY'),numInput(q,'max_alleles_y',{int:true}))),
  box(T('amLadderSpike'),el('div',{},radio('amLs',sp,'enabled',true,T('amEnable')),radio('amLs',sp,'enabled',false,T('amDisable'))),row(T('amLadderSpikeCut'),numInput(sp,'cutoff'))),
  box(T('amSampleSpike'),el('div',{},radio('amSs',q,'sample_spike',true,T('amEnable')),radio('amSs',q,'sample_spike',false,T('amDisable'))))]}
function tabSqGq(m){const s=m.sq_gq;
 const w=el('div',{class:'wgrid'});for(const [k,label] of WEIGHTS)w.append(el('label',{text:T(label)}),numInput(s.weights,k));
 const lw=el('div',{class:'wgrid'});
 const sel=(key)=>{const x=el('select');x.append(new Option('1','1'),new Option('0','0'));x.value=String(s[key]);x.addEventListener('change',()=>{s[key]=+x.value});return x};
 lw.append(el('label',{text:T('amSpikes')}),sel('ladder_spike'),el('label',{text:T('amOffscale')}),sel('ladder_offscale'));
 const rng=(key,label)=>el('div',{class:'rng'},el('b',{text:label}),el('span',{text:' '+T('amPassRange')+' '}),numInput(s[key],'pass'),el('span',{text:' '+T('amTo1')+'   '+T('amLowRange')+' '+T('amFrom0to')+' '}),numInput(s[key],'low'));
 return [box(T('amGqWeighting'),w),box(T('amSqWeighting'),row(T('wBd'),numInput(s,'sq_weight'))),box(T('amLadderGqWeighting'),lw),
  box(T('amRanges'),rng('sq',T('amSizingQuality')),rng('gq',T('amGenotypeQuality')))]}

const TABS=[['general','amGeneral',tabGeneral],['allele','amAllele',tabAllele],['detector','amPeakDetector',tabPeakDetector],['quality','amPeakQuality',tabPeakQuality],['sqgq','amSqGq',tabSqGq]];
const FACTORY_KEYS={allele:'allele',detector:'peak_detector',quality:'peak_quality',sqgq:'sq_gq'};

// Opens the editor on a copy; onSave(method, previousName) is called after validation.
function openEditor(source,{title,onSave}={}){
 const m=clone(source);const previous=source.name;let tab='general';
 const d=dialog(title||T('amTitle'),b=>{b.classList.add('amdlg')},
  [[T('ok'),async()=>{try{
     m.name=(m.name||'').trim();
     const r=await post('/api/analysis-method',{action:'validate',method:m});
     if(r.method.name!==previous&&get(r.method.name)){message(fmt('methodNameExists',r.method.name));return false}
     if(previous&&get(previous)&&!(await ask(T('methodSaveWarning'))))return false;
     if(r.unsupported.length)message(fmt('amUnsupported',r.unsupported.join(', ')));
     onSave(r.method,previous);return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]]);
 const body=d.body;
 const draw=()=>{body.replaceChildren();const tabs=el('div',{class:'tabs dtabs'});
  for(const [id,key] of TABS){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key)});t.addEventListener('mousedown',()=>{tab=id;draw()});tabs.append(t)}
  const page=el('div',{class:'tabpage dpage'});const spec=TABS.find(x=>x[0]===tab);page.append(...spec[2](m,draw));
  if(FACTORY_KEYS[tab]){const fb=el('button',{class:'btn',type:'button',text:T('amFactoryDefaults')});
   fb.addEventListener('click',async()=>{const f=await factory();m[FACTORY_KEYS[tab]]=f[FACTORY_KEYS[tab]];if(tab==='detector')m.fixed=f.fixed;draw()});page.append(el('div',{class:'btnrow right'},fb))}
  body.append(tabs,page)};
 draw();return d}
function ask(text){return new Promise(res=>{dialog(T('alert'),b=>b.append(el('p',{style:'max-width:440px;white-space:pre-line',text})),
 [[T('ok'),()=>{res(true);return true},true],[T('cancel'),()=>{res(false);return true}]])})}

// ---------- manager ----------
function download(name,text,type){const url=URL.createObjectURL(new Blob([text],{type}));const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
function openManager(){
 let selected=null;
 const d=dialog(T('mgrTitle'),b=>{b.classList.add('mgrdlg')},[[T('mgrDone'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();
  const tabs=el('div',{class:'tabs dtabs'},el('div',{class:'tab on',text:T('mgrMethods')}));
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('colName'),T('colLastSaved'),T('amInstrument').replace(':',''),T('amDescription').replace(':','')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');for(const r of list()){const tr=el('tr',{class:r.method.name===selected?'sel':''});
   tr.append(el('td',{text:r.method.name}),el('td',{text:new Date(r.saved).toLocaleString()}),el('td',{text:r.method.instrument}),el('td',{text:r.method.description}));
   tr.addEventListener('mousedown',()=>{selected=r.method.name;tb.querySelectorAll('tr').forEach(x=>x.classList.toggle('sel',x===tr))});tr.addEventListener('dblclick',()=>openSel());tb.append(tr)}
  t.append(tb);
  const btn=(label,fn,id)=>{const x=el('button',{class:'btn',type:'button',text:label,...(id?{id}:{})});x.addEventListener('click',fn);return x};
  const openSel=()=>{const m=get(selected);if(!m)return message(T('selectRow'));openEditor(m,{onSave:(mm,prev)=>{put(mm,prev);selected=mm.name;draw()}})};
  b.append(tabs,el('div',{class:'grid mgrgrid'},t),el('div',{class:'btnrow'},
   btn(T('mgrNew'),async()=>{const f=await factory();let n=1;while(get(`${T('mgrUntitled')} ${n}`))n++;f.name=`${T('mgrUntitled')} ${n}`;
    openEditor(f,{title:T('amNewTitle'),onSave:(mm)=>{put(mm);selected=mm.name;draw()}})},'mgrNewBtn'),
   btn(T('mgrOpen'),openSel),
   btn(T('mgrSaveAs'),()=>{const m=get(selected);if(!m)return message(T('selectRow'));
    dialog(T('mgrSaveAs'),bb=>bb.append(row(T('amName'),el('input',{type:'text',id:'saName',value:m.name+' copy'}))),
     [[T('ok'),bb=>{const n=bb.querySelector('#saName').value.trim();if(!n)return false;if(get(n)){message(fmt('methodNameExists',n));return false}
       put({...clone(m),name:n});selected=n;draw()},true],[T('cancel'),()=>true]])}),
   btn(T('mgrImport'),()=>{const i=el('input',{type:'file',accept:'.xml,.json',style:'display:none'});
    i.addEventListener('change',async()=>{const f=i.files[0];i.remove();if(!f)return;try{const text=await f.text();
     const r=/^\s*[{]/.test(text)?await post('/api/analysis-method',{action:'validate',method:JSON.parse(text)}):await post('/api/analysis-method',{action:'import_xml',xml:text});
     let n=r.method.name,k=2;while(get(n))n=`${r.method.name} (${k++})`;r.method.name=n;put(r.method);selected=n;draw();
     message(T('methodImported')+(r.unsupported.length?'\n'+fmt('amUnsupported',r.unsupported.join(', ')):''))}catch(e){message(e.message)}});
    document.body.append(i);i.click()},'mgrImportBtn'),
   btn(T('mgrExport'),async()=>{const m=get(selected);if(!m)return message(T('selectRow'));try{const r=await post('/api/analysis-method',{action:'export_xml',method:m});
    download(m.name+'.xml',r.xml,'application/xml')}catch(e){message(e.message)}}),
   btn(T('mgrDelete'),()=>{if(!selected)return message(T('selectRow'));if(list().length<=1)return message(T('mgrKeepOne'));
    ask(fmt('confirmDeleteMethod',selected)).then(ok=>{if(ok){remove(selected);selected=null;draw()}})})))};
 draw();return d}

window.Methods={init(p){post=p},list,get,put,remove,ensureDefault,factory,openEditor,openManager};
})();
