// OpenTrace Project window: samples, analysis summary and genotypes.
(function(){
const $=id=>document.getElementById(id);
const TOKEN=window.OT_TOKEN;

// Size standard library (fragment sizes are published product facts).
const SIZE_STANDARDS={
 'GS600_LIZ_(60-460)':[60,80,100,114,120,140,160,180,200,214,220,240,250,260,280,300,314,320,340,360,380,400,414,420,440,460],
 'GS600_LIZ':[80,100,114,120,140,160,180,200,214,220,240,250,260,280,300,314,320,340,360,380,400,414,420,440,460],
 'GS600_LIZ_(80-400)':[80,100,114,120,140,160,180,200,214,220,240,250,260,280,300,314,320,340,360,380,400],
 'GS500_LIZ_(75-450)':[75,100,139,150,160,200,300,340,350,400,450],
};
const EXAMPLE_METHOD='OpenTrace example';
const defaultMethod=()=>{let v=null;try{v=localStorage.getItem('opentrace.lastMethod')}catch(e){}
 return (v&&Methods.get(v))?v:(Methods.list()[0]?.method.name||'')};

const state={
 project:{name:null,runFolders:[],samples:[],dirty:false},
 kit:null,ladderQuality:null,markerQuality:null,
 tab:'samples',selected:new Set(),anchor:null,filter:null,lowQualityTop:false,
 showNavigator:true,busy:false,stop:false,nextId:1,
};
const kitCache=new Map(); // panel key + method -> {kit, lq, mq, policy}
const settingsCache=new Map(); // method + size standard -> analyze() settings
const EXAMPLE_KEY='__example__';
function dropCaches(){kitCache.forEach((v,k)=>{if(k!==EXAMPLE_KEY)kitCache.delete(k)});settingsCache.clear()}
window.OT={state,plotPayload,kitsChanged:()=>{dropCaches();render()}};
window.addEventListener('opentrace-methods',()=>{dropCaches();
 for(const s of state.project.samples)if(s.method!==EXAMPLE_METHOD&&!Methods.get(s.method))s.method=defaultMethod();render()});
const panelChoices=()=>KitStore.choices(KitStore.load());
function stdCovers(std,lo,hi){const f=SIZE_STANDARDS[std];return f[0]<=lo&&f[f.length-1]>=hi}
function panelRange(key){const [kitName,panel]=key.split('\u0001');const k=KitStore.load().kits.find(x=>x.name===kitName);const p=k?.panels.find(x=>x.name===panel);
 if(!p||!p.markers.length)return null;return [Math.min(...p.markers.map(m=>m.min_bp)),Math.max(...p.markers.map(m=>m.max_bp))]}
function fitStandard(key,current){const r=panelRange(key);if(!r||stdCovers(current,r[0],r[1]))return current;
 return Object.keys(SIZE_STANDARDS).find(s=>stdCovers(s,r[0],r[1]))||current}
const kitKey=(panelKey,method)=>panelKey===EXAMPLE_KEY?EXAMPLE_KEY:panelKey+'\u0002'+method;
async function kitFor(panelKey,methodName){
 const key=kitKey(panelKey,methodName);
 if(kitCache.has(key))return kitCache.get(key);
 const method=Methods.get(methodName);if(!method)throw Error(T('methodMissing')+' '+methodName);
 const req=KitStore.request(KitStore.load(),panelKey,method.allele.bin_set);if(!req)throw Error(T('panelNotReady'));
 const r=await jsonPost('/api/import-gm-kit',{panels:req.panels,bins:req.bins,stutter:req.stutter,panel:req.panel,analysis_method:method});
 const entry={kit:r.kit,lq:r.ladder_quality_definition,mq:r.marker_quality_definition,policy:'forward_fill',
  binSet:req.binSet,binSetSubstituted:req.binSetSubstituted};kitCache.set(key,entry);return entry}
const sampleKit=s=>kitCache.get(kitKey(s.panelKey,s.method))?.kit||null;
async function analyzeSettings(methodName,sizeStd){
 const key=methodName+'\u0001'+sizeStd;if(settingsCache.has(key))return settingsCache.get(key);
 const method=Methods.get(methodName);if(!method)throw Error(T('methodMissing')+' '+methodName);
 const r=await jsonPost('/api/analysis-method',{action:'analyze_settings',method,standard_bp:SIZE_STANDARDS[sizeStd]});
 const q={};for(const [k,v] of Object.entries(r.settings)){if(v===null||v===undefined)continue;q[k]=typeof v==='object'?JSON.stringify(v):String(v)}
 settingsCache.set(key,q);return q}
function methodSelect(value,onChange){const sel=el('select');
 for(const r of Methods.list())sel.append(new Option(r.method.name,r.method.name));
 if(value===EXAMPLE_METHOD)sel.append(new Option(EXAMPLE_METHOD,EXAMPLE_METHOD));
 sel.append(new Option(T('newMethodItem'),'__new__'));sel.value=value||'';
 sel.addEventListener('change',()=>{if(sel.value==='__new__'){sel.value=value||'';newMethod(onChange);return}onChange(sel.value)});
 sel.addEventListener('mousedown',e=>e.stopPropagation());return sel}
async function newMethod(after){try{const f=await Methods.factory();let n=1;while(Methods.get(`${T('mgrUntitled')} ${n}`))n++;f.name=`${T('mgrUntitled')} ${n}`;
 Methods.openEditor(f,{title:T('amNewTitle'),onSave:(m)=>{Methods.put(m);after&&after(m.name)}})}catch(e){message(e.message)}}
function editMethod(name){const m=Methods.get(name||defaultMethod());if(!m)return message(T('methodMissing')+' '+name);
 Methods.openEditor(m,{onSave:(mm,prev)=>{Methods.put(mm,prev);for(const s of state.project.samples)if(s.method===prev){s.method=mm.name;if(s.status==='analyzed')s.status='changed'}
  state.project.dirty=true;render()}})}
function panelSelect(value,onChange){const sel=el('select');sel.append(new Option(T('noPanelChoice'),''));
 const ch=panelChoices();for(const c of ch){const o=new Option(c.label,c.key);o.title=`${c.kit} / ${c.binSets.join(', ')}`;sel.append(o)}
 if(value===EXAMPLE_KEY)sel.append(new Option(state.kit?.name||'example',EXAMPLE_KEY));
 sel.value=value||'';sel.addEventListener('change',()=>onChange(sel.value));sel.addEventListener('mousedown',e=>e.stopPropagation());return sel}

async function jsonPost(url,data){
 const r=await fetch(url,{method:'POST',headers:{'X-Session-Token':TOKEN,'Content-Type':'application/json'},body:JSON.stringify(data)});
 const j=await r.json();if(!r.ok)throw Error(j.error||'Error');return j}
function compactAnalysis(a){const out={};for(const k of ['source_sha256','analysis_version','peak_settings','sizing','standard_definition_context','pullup_detection'])
 if(a[k]!==undefined)out[k]=a[k];out.channels=a.channels.map(c=>({tag:c.tag,group:c.group,peaks:c.peaks}));return out}

// ---------- quality mapping ----------
const ranges={pass:.75,low:.25};
const cat=v=>v==null?'na':v>=ranges.pass?'pass':v<=ranges.low?'low':'check';
const code=c=>c==null?'na':c===0?'pass':c===1?'check':c===2?'low':'na';
function samplePqv(s){
 const q={SOS:'na',SQ:'na',SSPK:'na',MIX:'na',OMR:'na',CGQ:'na'};
 const a=s.analysis;if(!a||s.status==='unanalyzed')return q;
 const sz=a.sizing||{};
 q.SQ=sz.status==='accepted'?cat((sz.post_sizing_quality||sz.base_quality||{}).score??1):sz.status?'low':'na';
 if(q.SQ==='low')return q; // samples failing sizing are not genotyped
 const peaks=(a.channels||[]).flatMap(c=>c.peaks||[]);
 q.SOS=peaks.some(p=>p.offscaled)?'check':'pass';
 if(peaks.some(p=>p.spike_classification))q.SSPK=peaks.some(p=>p.spike)?'check':'pass';
 const rep=s.report;
 if(s.type==='ladder'){const lq=s.ladderQuality;q.CGQ=lq==null?'na':lq===0?'pass':'low'}
 else if(rep){const gq=rep.quality?.marker_gq;if(gq)q.CGQ=code(gq.sample_category)}
 return q}
function markerRows(s){
 const rep=s.report;if(!rep||!rep.allele_calling)return [];
 const gq=Object.fromEntries((rep.quality?.marker_gq?.markers||[]).map(m=>[m.marker,m]));
 return (rep.allele_calling.markers||[]).map(m=>{
  const calls=(rep.calls||[]).filter(c=>c.marker===m.name);
  const g=gq[m.name];const score=g?.score?.gq;
  return {sample:s,marker:m.name,channel:m.channel,calls,gq:score,gqCat:g?cat(score):'na',status:m.status}})}

// ---------- tabs ----------
const TABS=[['samples','samples'],['summary','analysisSummary'],['genotypes','genotypes']];
function renderTabs(){const t=$('tabs');t.replaceChildren();
 for(const [id,key] of TABS){const b=el('div',{class:'tab'+(state.tab===id?' on':''),role:'tab',text:T(key)});b.addEventListener('mousedown',()=>showTab(id));t.append(b)}
 for(const [id] of TABS)$('page-'+id).hidden=state.tab!==id}
function showTab(id){state.tab=id;renderTabs();render()}

// ---------- navigator ----------
function currentKit(){const sel=state.project.samples.find(s=>state.selected.has(s.id)&&sampleKit(s));
 return sel?sampleKit(sel):(state.project.samples.map(sampleKit).find(Boolean)||null)}
function renderNavigator(){
 const nav=$('navigator');nav.classList.toggle('hidden',!state.showNavigator);nav.replaceChildren();
 const root=el('ul',{class:'tree root'});const li=el('li');
 const pnode=el('span',{class:'tnode'+(state.filter===null?' sel':'')});pnode.innerHTML=icon('project');pnode.append(document.createTextNode(T('project')));
 pnode.addEventListener('click',()=>{state.filter=null;render()});li.append(pnode);
 const ul=el('ul',{class:'tree'});
 if(state.tab==='genotypes'){
  for(const m of (currentKit()?.markers||[])){const n=el('span',{class:'tnode'});n.innerHTML=icon('marker');n.append(document.createTextNode(m.name));ul.append(el('li',{},n))}
 }else for(const rf of state.project.runFolders){
  const f=el('li');const fn=el('span',{class:'tnode'+(state.filter?.run===rf?' sel':'')});fn.innerHTML=icon('folder');fn.append(document.createTextNode(rf));
  fn.addEventListener('click',()=>{state.filter={run:rf};render()});f.append(fn);
  const su=el('ul',{class:'tree'});
  for(const s of state.project.samples.filter(x=>x.runFolder===rf)){
   const sn=el('span',{class:'tnode'+(state.selected.has(s.id)?' sel':'')});sn.innerHTML=icon('sample');sn.append(document.createTextNode(s.name));
   sn.addEventListener('click',()=>{state.selected=new Set([s.id]);render()});su.append(el('li',{},sn))}
  f.append(su);ul.append(f)}
 li.append(ul);root.append(li);nav.append(root)}

// ---------- samples table ----------
const TYPE_KEYS={ladder:'tAllelicLadder',sample:'tSample',positive:'tPositive',negative:'tNegative'};
function visibleSamples(){
 let list=state.project.samples;
 if(state.filter?.run)list=list.filter(s=>s.runFolder===state.filter.run);
 if(state.filter?.ids)list=list.filter(s=>state.filter.ids.has(s.id));
 if(state.lowQualityTop){const rank=s=>{const q=samplePqv(s);return Math.max(...Object.values(q).map(v=>({low:3,check:2,pass:1,na:0}[v]||0)))};
  list=[...list].sort((a,b)=>rank(b)-rank(a))}
 return list}
function statusIcon(s){
 if(s.status==='analyzed')return '';
 const color=s.status==='changed'?'#e5a000':'#2ea043';
 return `<svg viewBox="0 0 14 14" width="14" height="14"><path d="M3 2l9 5-9 5z" fill="${color}" stroke="#555"/></svg>`}
function renderSamples(){
 const grid=$('samplesGrid');grid.replaceChildren();
 const cols=[['',28],[T('colStatus'),48],[T('colSampleFile'),170],[T('colSampleName'),130],[T('colSampleType'),160],[T('colAnalysisMethod'),130],
  [T('colPanel'),170],[T('colSizeStandard'),130],['SOS',38],['SQ',38],['SSPK',42],['MIX',38],['OMR',42],['CGQ',42]];
 const t=el('table',{class:'g'});const cg=el('colgroup');for(const [,w] of cols)cg.append(el('col',{style:`width:${w}px`}));t.append(cg);
 const hr=el('tr');for(const [h] of cols)hr.append(el('th',{text:h,title:h}));t.append(el('thead',{},hr));
 const tb=el('tbody');const list=visibleSamples();
 list.forEach((s,i)=>{
  const tr=el('tr',{class:state.selected.has(s.id)?'sel':''});tr.dataset.id=s.id;
  tr.append(el('td',{class:'rh',text:String(i+1)}));
  const st=el('td',{class:'ic'});st.innerHTML=statusIcon(s);st.title=T(s.status==='analyzed'?'stAnalyzed':s.status==='changed'?'stChanged':'stUnanalyzed');tr.append(st);
  tr.append(el('td',{text:s.file,title:s.file}),el('td',{text:s.name}));
  const tsel=el('select');for(const k of ['sample','ladder','positive','negative'])tsel.append(new Option(T(TYPE_KEYS[k]),k));tsel.value=s.type;
  tsel.addEventListener('change',()=>{s.type=tsel.value;if(s.status==='analyzed')s.status='changed';state.project.dirty=true;render()});
  tsel.addEventListener('mousedown',e=>e.stopPropagation());
  const psel=panelSelect(s.panelKey,v=>{s.panelKey=v;s.panel=v?v.split('\u0001')[1]:'';if(v)s.sizeStd=fitStandard(v,s.sizeStd);
   if(s.status==='analyzed')s.status='changed';state.project.dirty=true;render()});
  const msel=methodSelect(s.method,v=>{s.method=v;try{localStorage.setItem('opentrace.lastMethod',v)}catch(e){}
   if(s.status==='analyzed')s.status='changed';state.project.dirty=true;render()});
  const mcell=el('td',{},msel);mcell.addEventListener('dblclick',e=>{e.stopPropagation();if(s.method!==EXAMPLE_METHOD)editMethod(s.method)});
  tr.append(el('td',{},tsel),mcell,el('td',{},psel));
  const ssel=el('select');for(const k of Object.keys(SIZE_STANDARDS))ssel.append(new Option(k,k));ssel.value=s.sizeStd;
  ssel.addEventListener('change',()=>{s.sizeStd=ssel.value;if(s.status==='analyzed')s.status='changed';state.project.dirty=true;render()});
  ssel.addEventListener('mousedown',e=>e.stopPropagation());
  tr.append(el('td',{},ssel));
  const q=samplePqv(s);for(const k of ['SOS','SQ','SSPK','MIX','OMR','CGQ']){const c=el('td',{class:'ic'});c.innerHTML=pqv(q[k]);tr.append(c)}
  tr.addEventListener('mousedown',e=>selectRow(e,s.id,list.map(x=>x.id)));
  tr.addEventListener('dblclick',()=>displayPlots());
  tb.append(tr)});
 t.append(tb);grid.append(t);
 if(!list.length)grid.append(el('div',{class:'empty',text:state.project.name?'':T('untitled')}));
}
function selectRow(e,id,order){
 if(e.shiftKey&&state.anchor!=null){const a=order.indexOf(state.anchor),b=order.indexOf(id);const [lo,hi]=a<b?[a,b]:[b,a];
  state.selected=new Set(order.slice(lo,hi+1))}
 else if(e.ctrlKey||e.metaKey){state.selected.has(id)?state.selected.delete(id):state.selected.add(id);state.anchor=id}
 else{state.selected=new Set([id]);state.anchor=id}
 render()}

// ---------- genotypes table ----------
function renderGenotypes(){
 const grid=$('genotypesGrid');grid.replaceChildren();
 const cols=[['',28],[T('colSampleFile'),160],[T('colSampleName'),120],[T('colMarker'),90],[T('colDye'),40],[T('colAllele')+' 1',60],[T('colAllele')+' 2',60],
  [T('colSize')+' 1',62],[T('colSize')+' 2',62],[T('colHeight')+' 1',62],[T('colHeight')+' 2',62],['OS',34],['BIN',34],['PHR',34],['LPH',34],['MPH',34],['SPK',34],['AN',34],['BD',34],['CC',34],['OVL',34],['GQ',40]];
 const t=el('table',{class:'g'});const cg=el('colgroup');for(const [,w] of cols)cg.append(el('col',{style:`width:${w}px`}));t.append(cg);
 const hr=el('tr');for(const [h] of cols)hr.append(el('th',{text:h,title:h}));t.append(el('thead',{},hr));
 const tb=el('tbody');let n=0;
 const samples=visibleSamples().filter(s=>s.type!=='ladder'||true);
 for(const s of samples)for(const r of markerRows(s)){
  const tr=el('tr',{class:state.selected.has(s.id)?'sel':''});n++;
  const a=r.calls[0],b=r.calls[1];const dye=dyeLetter(r.channel);
  tr.append(el('td',{class:'rh',text:String(n)}),el('td',{text:s.file}),el('td',{text:s.name}),el('td',{text:r.marker}),el('td',{text:dye}),
   el('td',{text:a?.label||''}),el('td',{text:b?.label||''}),el('td',{class:'num',text:a?a.size_bp.toFixed(2):''}),el('td',{class:'num',text:b?b.size_bp.toFixed(2):''}),
   el('td',{class:'num',text:a?String(Math.round(a.height)):''}),el('td',{class:'num',text:b?String(Math.round(b.height)):''}));
  for(let i=0;i<10;i++){const c=el('td',{class:'ic'});c.innerHTML=pqv('na');tr.append(c)}
  const g=el('td',{class:'ic'});g.innerHTML=pqv(r.gqCat);g.title=r.gq==null?'':String(r.gq);tr.append(g);
  tr.addEventListener('mousedown',e=>selectRow(e,s.id,samples.map(x=>x.id)));
  tr.addEventListener('dblclick',()=>displayPlots());
  tb.append(tr)}
 t.append(tb);grid.append(t)}
function dyeLetter(tag){const i={DATA1:'B',DATA2:'G',DATA3:'Y',DATA4:'R'}[tag];return i||(tag==='DATA105'?'P':tag==='DATA106'?'O':'')}

// ---------- analysis summary ----------
function renderSummary(){
 const box=$('summary');box.replaceChildren();const all=state.project.samples;
 box.append(el('h2',{text:T('analysisSummary')}));
 const link=(ids)=>{const a=el('a',{text:String(ids.length)});if(ids.length)a.addEventListener('click',()=>{state.filter={ids:new Set(ids)};showTab('samples')});return a};
 const st=el('table');st.append(el('tr',{},el('th',{text:T('sumStatus')}),el('th',{text:T('sumTotal')})));
 for(const [k,key] of [['unanalyzed','stUnanalyzed'],['analyzed','stAnalyzed'],['changed','stChanged']]){
  const ids=all.filter(s=>s.status===k).map(s=>s.id);const c=el('td');c.innerHTML=statusIcon({status:k})+' ';c.append(T(key));st.append(el('tr',{},c,el('td',{},link(ids))))}
 box.append(st,el('p',{text:T('sumIntro')}),el('p',{text:T('sumClick')}));
 const done=all.filter(s=>s.status!=='unanalyzed');
 const head=(first,cols)=>{const r=el('tr',{},el('th',{text:first}));for(const c of cols){const th=el('th');th.innerHTML=c;r.append(th)}return r};
 const worst=s=>{const v=Object.values(samplePqv(s));return v.includes('low')?'low':v.includes('check')?'check':'pass'};
 box.append(el('h3',{text:T('sumLadder')}),el('p',{text:T('sumLadderNote')}));
 const lt=el('table');lt.append(head(T('sumRunFolder'),[T('sumTotal'),pqv('pass'),pqv('check'),pqv('low')]));
 for(const rf of state.project.runFolders){const l=done.filter(s=>s.runFolder===rf&&s.type==='ladder');
  lt.append(el('tr',{},el('td',{text:rf}),el('td',{},link(l.map(s=>s.id))),...['pass','check','low'].map(k=>el('td',{},link(l.filter(s=>worst(s)===k).map(s=>s.id))))))}
 box.append(lt);
 box.append(el('h3',{text:T('sumControl')}));
 const ct=el('table');ct.append(head(T('sumControlType'),[T('sumTotal'),pqv('pass')+' '+T('sumAllMet'),pqv('low')+' '+T('sumOneNotMet')]));
 const ctrl=done.filter(s=>s.type==='positive'||s.type==='negative');
 for(const [label,list] of [[T('sumTotalRow'),ctrl],[T('tPositive'),ctrl.filter(s=>s.type==='positive')],[T('tNegative'),ctrl.filter(s=>s.type==='negative')]])
  ct.append(el('tr',{},el('td',{text:label}),el('td',{},link(list.map(s=>s.id))),el('td',{},link(list.filter(s=>worst(s)==='pass').map(s=>s.id))),el('td',{},link(list.filter(s=>worst(s)!=='pass').map(s=>s.id)))));
 box.append(ct);
 box.append(el('h3',{text:T('sumSample')}));
 const smp=done.filter(s=>s.type==='sample');
 const stt=el('table');stt.append(head('',[T('sumTotal'),pqv('pass')+' '+T('sumAllMet'),pqv('low')+' '+T('sumOneNotMet')]));
 stt.append(el('tr',{},el('td',{text:T('samples')}),el('td',{},link(smp.map(s=>s.id))),el('td',{},link(smp.filter(s=>worst(s)==='pass').map(s=>s.id))),el('td',{},link(smp.filter(s=>worst(s)!=='pass').map(s=>s.id)))));
 box.append(stt)}

// ---------- render ----------
function render(){
 document.title=`${T('app')} - ${state.project.name?(state.project.dirty?'*':'')+state.project.name:T('untitled')}`;
 renderNavigator();
 if(state.tab==='samples')renderSamples();else if(state.tab==='summary')renderSummary();else renderGenotypes();
 const any=state.project.samples.length>0;
 for(const id of ['tbAnalyze','tbPlots','tbExport'])if($(id))$(id).disabled=!any||state.busy;
 $('stopBtn').disabled=!state.busy;
 if(!state.busy)$('statusText').textContent=state.lastMessage||`${state.project.samples.length} ${T('samples').toLowerCase()}`;
}
function progress(done,total,text){const pct=total?Math.round(100*done/total):0;$('progressBar').style.width=pct+'%';$('progressText').textContent=pct+'%';if(text)$('statusText').textContent=text}

// ---------- actions ----------
const notYet=()=>message(T('notImplemented'));
function newProject(){
 dialog(T('newProjectTitle'),b=>{b.append(el('div',{class:'field'},el('label',{text:T('projectName'),for:'npName'}),el('input',{type:'text',id:'npName',value:''})))},
  [[T('ok'),body=>{const v=body.querySelector('#npName').value.trim();if(!v)return false;
    Object.assign(state.project,{name:v,runFolders:[],samples:[],dirty:true});state.selected.clear();state.filter=null;render()},true],[T('cancel'),()=>true]])}
// File-name heuristics for sample type; the user can change the type in the table.
function sampleTypeFromName(n){
 if(/ladder|allelic|_al_|(^|[_-])lad([_-]|$)|^L\d*[_-]/i.test(n))return 'ladder';
 if(/neg|^NC\d*[_-]|[_-]NC\d*[_-]|blank/i.test(n))return 'negative';
 if(/pos|^PC\d*[_-]|[_-]PC\d*[_-]|control|9947|2800M|(^|[_-])007([_-]|$)/i.test(n))return 'positive';
 return 'sample'}
function addSamples(){
 if(!state.project.name)Object.assign(state.project,{name:T('untitled'),dirty:true});
 let files=[];
 dialog(T('addTitle'),b=>{
  const list=el('div',{class:'filelist'});const input=el('input',{type:'file',multiple:'',accept:'.fsa,.ab1',style:'display:none'});
  const redraw=()=>{list.replaceChildren(...files.map(f=>el('div',{text:f.name})))};
  input.addEventListener('change',()=>{files=[...files,...input.files];input.value='';redraw()});
  const choose=el('button',{class:'btn',text:T('addChoose'),type:'button'});choose.addEventListener('click',()=>input.click());
  const clear=el('button',{class:'btn',text:T('addClear'),type:'button'});clear.addEventListener('click',()=>{files=[];redraw()});
  const ss=el('select',{id:'addStd'});for(const k of Object.keys(SIZE_STANDARDS))ss.append(new Option(k,k));
  let lastKey='';try{lastKey=localStorage.getItem('opentrace.lastPanel')||''}catch(e){}
  if(!panelChoices().some(c=>c.key===lastKey))lastKey='';
  const ps=panelSelect(lastKey,v=>{if(v)ss.value=fitStandard(v,ss.value)});ps.id='addPanel';if(lastKey)ss.value=fitStandard(lastKey,ss.value);
  const fs=el('fieldset',{},el('legend',{text:T('addSettings')}),
   el('div',{class:'field'},el('label',{text:T('runFolderName')}),el('input',{type:'text',id:'addRun',value:'Run '+(state.project.runFolders.length+1)})),
   el('div',{class:'field'},el('label',{text:T('colAnalysisMethod')}),(()=>{const m=methodSelect(defaultMethod(),v=>{m.value=v});m.id='addMethod';return m})()),
   el('div',{class:'field'},el('label',{text:T('colPanel')}),ps),
   el('div',{class:'field'},el('label',{text:T('colSizeStandard')}),ss));
  if(!panelChoices().length)fs.append(el('div',{class:'note',text:T('noPanelsYet')}));
  b.append(el('div',{text:T('addFiles')}),list,el('div',{style:'display:flex;gap:6px;margin:6px 0'},choose,clear,input),fs)},
  [[T('add'),body=>{if(!files.length)return false;const run=body.querySelector('#addRun').value.trim()||'Run';const std=body.querySelector('#addStd').value;const pkey=body.querySelector('#addPanel').value;const meth=body.querySelector('#addMethod').value;
    try{localStorage.setItem('opentrace.lastMethod',meth)}catch(e){}
    try{localStorage.setItem('opentrace.lastPanel',pkey)}catch(e){}
    if(!state.project.runFolders.includes(run))state.project.runFolders.push(run);
    for(const f of files){const name=f.name.replace(/\.(fsa|ab1)$/i,'');
     state.project.samples.push({id:state.nextId++,file:f.name,name,type:sampleTypeFromName(f.name),
      runFolder:run,method:meth,panelKey:pkey,panel:pkey?pkey.split('\u0001')[1]:'',sizeStd:std,status:'unanalyzed',blob:f,analysis:null,report:null})}
    state.project.dirty=true;render()},true],[T('cancel'),()=>true]])}

async function analyze(scope){
 const targets=state.project.samples.filter(s=>scope==='all'||(scope==='selected'?state.selected.has(s.id):s.status!=='analyzed'));
 if(!targets.length||state.busy)return;
 state.busy=true;state.stop=false;state.lastMessage=null;state.noLadder=null;state.binSetNote=null;render();
 const order=[...targets].sort((a,b)=>(a.type==='ladder'?0:1)-(b.type==='ladder'?0:1)); // ladders first
 let i=0;
 try{
  for(const s of order){
   if(state.stop)break;
   progress(i,order.length,`${T('analyzing')}: ${s.file}`);
   if(s.blob){const p=new URLSearchParams({...(await analyzeSettings(s.method,s.sizeStd)),filename:s.file});
    const r=await fetch('/api/analyze?'+p,{method:'POST',headers:{'X-Session-Token':TOKEN},body:s.blob});
    const j=await r.json();if(!r.ok)throw Error(`${s.file}: ${j.error}`);s.analysis=j}
   s.status='analyzed';i++;progress(i,order.length)}
  await genotype();
  const any=state.project.samples.some(s=>s.panelKey);
  state.lastMessage=state.noLadder?T('noLadderInRun')+' '+state.noLadder:any?T('analysisDone')+(state.binSetNote?' — '+T('binSetSubstituted')+' '+state.binSetNote:''):T('noKit');
 }catch(err){message(err.message)}
 finally{state.busy=false;state.project.dirty=true;render()}}

async function genotype(){
 for(const rf of state.project.runFolders){
  const inRun=state.project.samples.filter(s=>s.runFolder===rf&&s.analysis&&s.status==='analyzed'&&s.panelKey);
  for(const key of [...new Set(inRun.map(s=>s.panelKey))]){
   if(state.stop)return;
   const group=inRun.filter(s=>s.panelKey===key);
   const methods=[...new Set(group.filter(s=>s.type!=='ladder').map(s=>s.method))];
   const ladders=group.filter(s=>s.type==='ladder').map(s=>({ladder_id:s.file,analysis:s.compact||compactAnalysis(s.analysis)}));
   if(!ladders.length){for(const s of group)s.report=null;state.noLadder=rf;continue}
   for(const s of group)if(s.type==='ladder')s.ladderQuality=null;
   const kits={};for(const mname of methods)kits[mname]=await kitFor(key,mname);
   for(const s of group.filter(x=>x.type!=='ladder')){
    if(state.stop)return;
    const k=kits[s.method];if(k.binSetSubstituted)state.binSetNote=k.binSet;
    $('statusText').textContent=`${T('genotyping')}: ${s.file}`;
    let rep=await jsonPost('/api/str-run',{sample:s.compact||compactAnalysis(s.analysis),ladders,kit:k.kit,ladder_quality_definition:k.lq,missing_offset_policy:k.policy});
    if(k.mq)rep=await jsonPost('/api/marker-quality',{report:rep,definition:k.mq});
    s.report=rep;
    for(const l of rep.quality?.run_ladders||[]){const ls=group.find(x=>x.file===l.ladder_id);if(ls)ls.ladderQuality=l.quality_code}}}}}

async function loadExample(){
 if(state.busy)return;state.busy=true;render();progress(0,1,T('loadExample'));
 try{
  const d=await jsonPost('/api/demo-str-run',{});
  let rep=d.report;if(d.marker_quality_definition)rep=await jsonPost('/api/marker-quality',{report:rep,definition:d.marker_quality_definition});
  const rf='Example run';
  Object.assign(state.project,{name:'OpenTrace example',runFolders:[rf],samples:[],dirty:false});
  state.kit=d.kit;state.ladderQuality=d.ladder_quality_definition;state.markerQuality=d.marker_quality_definition;
  kitCache.set(EXAMPLE_KEY,{kit:d.kit,lq:d.ladder_quality_definition,mq:d.marker_quality_definition,policy:'reject'});
  for(const l of d.ladders){const lq=(rep.quality?.run_ladders||[]).find(x=>x.ladder_id===l.ladder_id);
   state.project.samples.push({id:state.nextId++,file:l.ladder_id,name:l.ladder_id.replace(/\.fsa$/,''),type:'ladder',runFolder:rf,method:EXAMPLE_METHOD,panelKey:EXAMPLE_KEY,panel:d.kit.name,
    sizeStd:'GS600_LIZ',status:'analyzed',analysis:l.analysis,compact:l.analysis,report:null,ladderQuality:lq?.quality_code})}
  state.project.samples.push({id:state.nextId++,file:d.sample.filename||'example-sample.fsa',name:d.sample.sample||'Example sample',type:'sample',runFolder:rf,
   method:EXAMPLE_METHOD,panelKey:EXAMPLE_KEY,panel:d.kit.name,sizeStd:'GS600_LIZ',status:'analyzed',analysis:d.sample,report:rep});
  state.selected.clear();state.filter=null;progress(1,1,T('ready'));
 }catch(err){message(err.message)}finally{state.busy=false;render()}}

function plotPayload(){
 const samples=state.project.samples.filter(s=>state.selected.has(s.id));
 return {lang:I18N.lang,kit:(samples.map(sampleKit).find(Boolean))||null,samples:samples.map(s=>({id:s.id,file:s.file,name:s.name,type:s.type,analysis:s.analysis,report:s.report,kit:sampleKit(s),pqv:samplePqv(s),
  markers:markerRows(s).map(r=>({marker:r.marker,channel:r.channel,gq:r.gq,gqCat:r.gqCat,calls:r.calls}))}))}}
function openPanelManager(){const w=window.open('/idx/panels.html','opentrace-panels','width=1180,height=800');if(!w)message(T('plotWindowBlocked'));else w.focus()}
function displayPlots(){
 if(!state.selected.size)return message(T('selectSamplesFirst'));
 const w=window.open('/idx/plot.html','opentrace-plot','width=1100,height=760');
 if(!w)message(T('plotWindowBlocked'));else w.focus()}

function exportTable(){
 const rows=[];const tab=state.tab==='genotypes'?'genotypes':'samples';
 if(tab==='samples'){rows.push(['Sample File','Sample Name','Sample Type','Analysis Method','Panel','Size Standard','SOS','SQ','SSPK','MIX','OMR','CGQ']);
  const m={pass:0,check:1,low:2,na:-2,override:0.5};
  for(const s of visibleSamples()){const q=samplePqv(s);rows.push([s.file,s.name,T(TYPE_KEYS[s.type]),s.method,s.panel,s.sizeStd,...['SOS','SQ','SSPK','MIX','OMR','CGQ'].map(k=>m[q[k]])])}}
 else{rows.push(['Sample File','Sample Name','Marker','Dye','Allele 1','Allele 2','Size 1','Size 2','Height 1','Height 2','GQ']);
  for(const s of visibleSamples())for(const r of markerRows(s)){const a=r.calls[0],b=r.calls[1];
   rows.push([s.file,s.name,r.marker,dyeLetter(r.channel),a?.label||'',b?.label||'',a?.size_bp??'',b?.size_bp??'',a?.height??'',b?.height??'',r.gq??''])}}
 const text=rows.map(r=>r.join('\t')).join('\r\n')+'\r\n';
 const url=URL.createObjectURL(new Blob([text],{type:'text/plain'}));const a=el('a',{href:url,download:`${state.project.name||'project'}_${tab}.txt`});a.click();setTimeout(()=>URL.revokeObjectURL(url),1000)}

function deleteFromProject(){
 state.project.samples=state.project.samples.filter(s=>!state.selected.has(s.id));
 state.project.runFolders=state.project.runFolders.filter(rf=>state.project.samples.some(s=>s.runFolder===rf));
 state.selected.clear();state.project.dirty=true;render()}
function selectAll(){state.selected=new Set(visibleSamples().map(s=>s.id));render()}
function setLang(l){I18N.set(l);buildChrome();render()}

// ---------- chrome ----------
function buildChrome(){
 document.documentElement.lang=I18N.lang;
 const lang=I18N.lang;
 buildMenubar($('menubar'),[
  {label:T('file'),items:[{label:T('newProject'),sc:'Ctrl+N',run:newProject},{label:T('openProject'),sc:'Ctrl+O',run:notYet},{label:T('closeProject'),run:()=>{Object.assign(state.project,{name:null,runFolders:[],samples:[]});render()}},
   {label:T('saveProject'),sc:'Ctrl+S',run:notYet},{label:T('saveProjectAs'),run:notYet},'-',
   {label:T('addSamples'),sc:'Ctrl+K',run:addSamples},{label:T('associateSamples'),disabled:true},'-',
   {label:T('exportTable'),sc:'Ctrl+E',run:exportTable},{label:T('exportCombined'),run:notYet},{label:T('exportCodis'),run:notYet},'-',
   {label:T('projectOptions'),run:notYet},'-',{label:T('pageSetup'),run:notYet},{label:T('printPreview'),run:notYet},{label:T('print'),sc:'Ctrl+P',run:()=>window.print()},'-',
   {label:T('logOut'),disabled:true},{label:T('exit'),run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('undo'),sc:'Ctrl+Z',disabled:true},{label:T('redo'),sc:'Ctrl+Y',disabled:true},'-',{label:T('copy'),sc:'Ctrl+C',run:notYet},{label:T('paste'),sc:'Ctrl+V',disabled:true},
   {label:T('clear'),disabled:true},{label:T('selectAll'),sc:'Ctrl+A',run:selectAll},{label:T('fillDown'),sc:'Ctrl+D',run:notYet},'-',{label:T('deleteFromProject'),run:deleteFromProject},'-',
   {label:T('find'),sc:'Ctrl+F',run:notYet},{label:T('sort'),run:notYet}]},
  {label:T('analysis'),items:[{label:T('analyze'),sc:'Ctrl+R',run:()=>analyze('pending')},{label:T('analyzeSelected'),run:()=>analyze('selected')},{label:T('analyzeAll'),run:()=>analyze('all')},'-',
   {label:T('lowQualityTop'),sc:'Ctrl+B',checked:state.lowQualityTop,run:()=>{state.lowQualityTop=!state.lowQualityTop;buildChrome();render()}},{label:T('nonConcordantTop'),disabled:true},'-',
   {label:T('displayPlots'),sc:'Ctrl+L',run:displayPlots},'-',{label:T('analysisMethodEditor'),run:()=>editMethod(state.project.samples.find(s=>state.selected.has(s.id))?.method)},{label:T('sizeStandardEditor'),run:notYet},{label:T('sizeMatchEditor'),run:notYet},{label:T('binOffsets'),run:notYet}]},
  {label:T('view'),items:[{label:T('samples'),checked:state.tab==='samples',run:()=>{buildAfter(()=>showTab('samples'))}},{label:T('analysisSummary'),checked:state.tab==='summary',run:()=>buildAfter(()=>showTab('summary'))},
   {label:T('genotypes'),checked:state.tab==='genotypes',run:()=>buildAfter(()=>showTab('genotypes'))},'-',{label:T('sampleInfo'),run:notYet},{label:T('rawData'),run:notYet},{label:T('eptData'),run:notYet},'-',
   {label:T('showNavigator'),checked:state.showNavigator,run:()=>{state.showNavigator=!state.showNavigator;buildChrome();render()}},{label:T('fitToContent'),run:notYet},'-',
   {label:T('language'),sub:[{label:'Русский',checked:lang==='ru',run:()=>setLang('ru')},{label:'English',checked:lang==='en',run:()=>setLang('en')}]}]},
  {label:T('tools'),items:[{label:T('manager'),sc:'Ctrl+M',run:()=>Methods.openManager()},{label:T('panelManager'),sc:'Ctrl+J',run:openPanelManager},{label:T('reportManager'),sc:'Ctrl+Q',run:notYet},{label:T('labelEditViewer'),run:notYet},
   {label:T('tableSettingEditor'),sc:'Ctrl+T',run:notYet},{label:T('codisManager'),run:notYet},'-',{label:T('profileManager'),run:notYet},{label:T('profileComparison'),run:notYet},'-',
   {label:T('mixture'),sc:'Ctrl+U',run:notYet},{label:T('mixtureManager'),run:notYet}]},
  {label:T('admin'),items:[{label:T('securityManager'),disabled:true},{label:T('auditManager'),sub:[{label:T('auditSetting'),disabled:true},{label:T('auditReport'),disabled:true}]},
   {label:T('esig'),disabled:true},{label:T('databaseManager'),disabled:true}]},
  {label:T('help'),items:[{label:T('helpTopics'),sc:'F1',run:notYet},{label:T('loadExample'),run:loadExample},'-',{label:T('about'),run:()=>message(T('aboutText'),T('about'))}]},
 ]);
 const tb=$('toolbar');tb.replaceChildren(
  tbutton('newProject',T('newProject'),newProject),tbutton('openProject',T('openProject'),notYet),tbutton('saveProject',T('saveProject'),notYet),tsep(),
  tbutton('addSamples',T('addSamples'),addSamples),tbutton('exportTable',T('exportTable'),exportTable,{id:'tbExport'}),tsep(),
  tbutton('displayPlots',T('displayPlots'),displayPlots,{id:'tbPlots'}),tbutton('reportManager',T('reportManager'),notYet),tbutton('labelEditViewer',T('labelEditViewer'),notYet),
  tbutton('sizeMatchEditor',T('sizeMatchEditor'),notYet),tsep(),tbutton('analysisMethodEditor',T('analysisMethodEditor'),()=>editMethod(state.project.samples.find(s=>state.selected.has(s.id))?.method),{id:'tbMethodEditor'}),tbutton('panelManager',T('panelManager'),openPanelManager,{id:'tbPanelManager'}),
  tbutton('manager',T('manager'),()=>Methods.openManager(),{id:'tbManager'}),tsep(),tbutton('analyze',T('analyze'),()=>analyze('pending'),{id:'tbAnalyze'}),tbutton('lowQualityTop',T('lowQualityTop'),()=>{state.lowQualityTop=!state.lowQualityTop;render()}),
  tsep(),el('span',{class:'tlabel',text:T('tableSetting')}));
 const ts=el('select',{class:'combo',title:T('tableSetting')});ts.append(new Option(T('dataInterpretation'),'default'));
 tb.append(ts,tbutton('tableSettingEditor',T('tableSettingEditor'),notYet),tsep(),tbutton('find',T('find'),notYet),tbutton('print',T('print'),()=>window.print()),
  tbutton('printPreview',T('printPreview'),notYet),tbutton('help',T('helpTopics'),notYet));
 $('stopBtn').textContent=T('stop');
 renderTabs()}
function buildAfter(fn){fn();buildChrome();render()}

$('stopBtn').addEventListener('click',()=>{state.stop=true});
document.addEventListener('keydown',e=>{
 if(e.target.closest('input,select,textarea'))return;
 const k=e.key.toLowerCase();
 if(e.key==='F1'){e.preventDefault();notYet();return}
 if(!e.ctrlKey&&!e.metaKey)return;
 const map={m:()=>Methods.openManager(),j:openPanelManager,n:newProject,k:addSamples,e:exportTable,l:displayPlots,r:()=>analyze('pending'),a:selectAll,b:()=>{state.lowQualityTop=!state.lowQualityTop;render()}};
 if(map[k]){e.preventDefault();map[k]()}});
window.addEventListener('beforeunload',e=>{if(state.project.dirty&&state.project.samples.length){e.preventDefault();e.returnValue=''}});

Methods.init(jsonPost);
buildChrome();render();
Methods.ensureDefault().then(()=>render()).catch(e=>message(e.message));
})();
