/* Hot keys follow the physical key (works with a Russian layout: Ctrl+В is Ctrl+D). */
var hotKey=e=>/^Key[A-Z]$/.test(e.code||'')?e.code.slice(3).toLowerCase():String(e.key||'').toLowerCase();
// OpenTrace Project window: samples, analysis summary and genotypes.
(function(){
const $=id=>document.getElementById(id);
const TOKEN=window.OT_TOKEN;
// Browser tab id (kept over a reload): the OpenTrace Server gives the edit lock of an opened project to one tab.
const TAB=(()=>{try{let t=sessionStorage.getItem('opentrace.tab');if(!t){t=Math.random().toString(36).slice(2)+Date.now().toString(36);sessionStorage.setItem('opentrace.tab',t)}return t}catch(e){return ''}})();

// Size standards come from the library (sizestd.js); this view maps name -> fragment list.
const SIZE_STANDARDS=new Proxy({},{get:(_,name)=>SizeStd.get(String(name))?.sizes,has:(_,name)=>!!SizeStd.get(String(name)),
 ownKeys:()=>SizeStd.names(),getOwnPropertyDescriptor:(_,name)=>SizeStd.get(String(name))?{enumerable:true,configurable:true,value:SizeStd.get(String(name)).sizes}:undefined});
const EXAMPLE_METHOD='OpenTrace example';
const defaultMethod=()=>{let v=null;try{v=localStorage.getItem('opentrace.lastMethod')}catch(e){}
 return (v&&Methods.get(v))?v:(Methods.list()[0]?.method.name||'')};

const state={
 project:{name:null,runFolders:[],samples:[],dirty:false},
 kit:null,ladderQuality:null,markerQuality:null,
 tab:'samples',selected:new Set(),genoSel:[],genoAnchor:null,anchor:null,filter:null,lowQualityTop:false,
 showNavigator:true,busy:false,stop:false,nextId:1,
};
const kitCache=new Map(); // panel key + method -> {kit, lq, mq, policy}
const settingsCache=new Map(); // method + size standard -> analyze() settings
const EXAMPLE_KEY='__example__';
function dropCaches(){kitCache.forEach((v,k)=>{if(k!==EXAMPLE_KEY)kitCache.delete(k)});settingsCache.clear()}
window.OT={state,plotPayload,manager:()=>Methods.openManager(),dropKitCache:()=>kitCache.clear(),addSampleFiles:(files,run,meth,pkey,std)=>addSampleFiles(files,run,meth,pkey,std),analyze:scope=>analyze(scope),printPayload:()=>printPayload(),loadExample:()=>loadExample(),displayPlots:()=>displayPlots(),writeProject:(n,o)=>writeProject(n,o),loadProject:n=>loadProject(n),closeProject:()=>closeProject(),report:null,edit:(id,spec)=>applyEdit(id,spec),editMany:(list,reason)=>applyEdits(list,reason),editItems:(id,spec)=>editItems(id,spec),peakRawData:(id,scan)=>{state.selected=new Set([id]);SampleViews.focusRaw(scan);buildAfter(()=>showTab('raw'))},labelEditViewer:()=>labelEditViewer(),
 // plot window: Edit > Undo (latest active label edit of the displayed samples), View > Sample Info / Raw Data / EPT Data
 undoLast:(ids)=>{const all=state.project.samples.filter(s=>ids.includes(s.id)).flatMap(s=>(s.edits||[]).filter(e=>!e.undone).map(e=>({s,e})));
  if(!all.length)return null;all.sort((a,b)=>String(a.e.at||a.e.time||'').localeCompare(String(b.e.at||b.e.time||''))||(a.e.id-b.e.id));const {s,e}=all[all.length-1];
  e.undone=true;e.undoneAt=new Date().toISOString();s.rev=(s.rev||0)+1;state.project.dirty=true;
  Sec.record([{map:'Allele',attribute:'modified',instance:`${s.file}/${e.marker||''}`,old:e.new||'',new:e.old||'',event:'Undo Edit'}]);render();return {sample:s.name,action:e.action,marker:e.marker||''}},
 exportWithStutter:(ids)=>exportWithStutter(ids),stutterRows:(ids)=>stutterRows(ids),pageSetupDialog:()=>pageSetupDialog(),showSampleView:(id,tab)=>{state.selected=new Set([id]);buildAfter(()=>showTab(tab))},pageSetup:()=>pageSetup(),kitsChanged:()=>{dropCaches();render()}};
window.addEventListener('opentrace-options',()=>render());
window.addEventListener('opentrace-tables',()=>{state.sort=null;buildChrome();render()});
window.addEventListener('opentrace-methods',()=>{dropCaches();
 for(const s of state.project.samples)if(s.method!==EXAMPLE_METHOD&&!Methods.get(s.method))s.method=defaultMethod();render()});
// kits whose security group the user may not read are hidden, as in the Panel Manager
const panelChoices=()=>{const db=KitStore.load();const ok=new Set(db.kits.filter(k=>Sec.objectCan(k.security_group,'read',k.owner)).map(k=>k.name));
 return KitStore.choices(db).filter(c=>ok.has(c.kit))};
function stdCovers(std,lo,hi){const f=SIZE_STANDARDS[std];return !!f&&f[0]<=lo&&f[f.length-1]>=hi}
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
// Kits (markers, bins) of the samples: the plot window draws the marker headers and the bins from them. They were built
// only by Analyze, so a project opened from the library or the server showed electropherograms without loci and bins
// (and the cache is dropped when the project brings its own panels). Built here in the background after opening and
// before plotting; the plot window is told when they are ready. Returns the panels that could not be built.
const kitsPending=new Map();
async function ensureKits(samples){const need=new Map();
 for(const s of samples){if(!s.panelKey||!s.method)continue;const k=kitKey(s.panelKey,s.method);if(!kitCache.has(k)&&!need.has(k))need.set(k,s)}
 const failed=[];let added=0;
 for(const [k,s] of need){let p=kitsPending.get(k);if(!p){p=kitFor(s.panelKey,s.method).finally(()=>kitsPending.delete(k));kitsPending.set(k,p)}
  try{await p;added++}catch(e){failed.push((s.panel||s.panelKey)+' / '+s.method+': '+e.message)}}
 if(added){render();try{const w=state.plotWin;if(w&&!w.closed&&w.otKitsReady)w.otKitsReady()}catch(e){}}
 return failed}
function ensureKitsQuiet(samples){ensureKits(samples).then(f=>{if(f.length){state.lastMessage=T('kitsNotBuilt')+' '+f.join('; ');render()}}).catch(()=>{})}
async function analyzeSettings(methodName,sizeStd){
 const key=methodName+'\u0001'+sizeStd;if(settingsCache.has(key))return settingsCache.get(key);
 const method=Methods.get(methodName);if(!method)throw Error(T('methodMissing')+' '+methodName);
 const std=SizeStd.get(sizeStd);if(!std)throw Error(T('arnmStandard')+': '+sizeStd);
 const r=await jsonPost('/api/analysis-method',{action:'analyze_settings',method,standard_bp:std.sizes,standard_dye:std.dye});
 const q={};for(const [k,v] of Object.entries(r.settings)){if(v===null||v===undefined)continue;q[k]=typeof v==='object'?JSON.stringify(v):String(v)}
 settingsCache.set(key,q);return q}
function methodSelect(value,onChange){const sel=el('select');
 for(const r of Methods.list())if(r.method.name===value||Sec.items.can('methods',r.method.name,'read'))sel.append(new Option(r.method.name,r.method.name));
 if(value===EXAMPLE_METHOD)sel.append(new Option(EXAMPLE_METHOD,EXAMPLE_METHOD));
 sel.append(new Option(T('newMethodItem'),'__new__'));sel.value=value||'';
 sel.addEventListener('change',()=>{if(sel.value==='__new__'){sel.value=value||'';newMethod(onChange);return}onChange(sel.value)});
 sel.addEventListener('mousedown',e=>e.stopPropagation());return sel}
async function newMethod(after){try{const f=await Methods.factory();let n=1;while(Methods.get(`${T('mgrUntitled')} ${n}`))n++;f.name=`${T('mgrUntitled')} ${n}`;
 Methods.openEditor(f,{title:T('amNewTitle'),onSave:(m)=>{Methods.put(m);after&&after(m.name)}})}catch(e){message(e.message)}}
function editMethod(name){const m=Methods.get(name||defaultMethod());if(!m)return message(T('methodMissing')+' '+name);if(!Sec.need('READ_ANALYSIS_METHOD'))return;
 if(!Sec.items.need('methods',m.name,'read'))return;
 const restrict=!Sec.items.can('methods',m.name,'update')?'read':Sec.can('UPDATE_ANALYSIS_METHOD')?null:(Sec.can('UPDATE_ANALYSIS_RANGE')||Sec.can('UPDATE_SIZING_RANGE'))?'ranges':'read';
 Methods.openEditor(m,{restrict,onSave:(mm,prev)=>{Methods.put(mm,prev);for(const s of state.project.samples)if(s.method===prev){s.method=mm.name;if(s.status==='analyzed')s.status='changed'}
  state.project.dirty=true;render()}})}
function panelSelect(value,onChange){const sel=el('select');sel.append(new Option(T('noPanelChoice'),''));
 const ch=panelChoices();for(const c of ch){const o=new Option(c.label,c.key);o.title=`${c.kit} / ${c.binSets.join(', ')}`;sel.append(o)}
 if(value===EXAMPLE_KEY)sel.append(new Option(state.kit?.name||'example',EXAMPLE_KEY));
 sel.value=value||'';sel.addEventListener('change',()=>onChange(sel.value));sel.addEventListener('mousedown',e=>e.stopPropagation());return sel}

// Large request bodies (sample saves: ~1 MB of JSON) go gzip-compressed (~8x smaller) once the server has said it
// accepts that (OpenTrace Server sends X-OT-Accepts-Gzip); the desktop app and the stand-alone web app never do.
let gzipUploads=false;
const gzipText=s=>new Response(new Blob([s]).stream().pipeThrough(new CompressionStream('gzip'))).arrayBuffer();
// Drawing state that older plot windows wrote onto shared kit objects (`_hit`) never goes to the core or the server.
const dropDrawingState=(k,v)=>k==='_hit'?undefined:v;
async function jsonPost(url,data){
 const text=JSON.stringify(data,dropDrawingState),headers={'X-Session-Token':TOKEN,'X-User-Ticket':Sec.ticket(),'X-OT-Tab':TAB,'Content-Type':'application/json'};let body=text;
 if(gzipUploads&&text.length>=65536&&typeof CompressionStream==='function'){try{body=await gzipText(text);headers['Content-Encoding']='gzip'}catch(e){body=text}}
 let r;try{r=await fetch(url,{method:'POST',headers,body});if(r.headers.get('x-ot-accepts-gzip')==='1')gzipUploads=true}
 catch(e){throw Error(T('serverUnreachable').replace('{0}',url)+' ('+e.message+')')}
 let j;try{j=await r.json()}catch(e){throw Error(`${url}: HTTP ${r.status}`)}
 if(!r.ok)throw Error(j.error||'Error');return j}
// Memory: a 48-file GlobalFiler folder held ~2 GB of per-attempt pull-up traces and ladder-matching details in the tab
// (renderer "Out of Memory"). Only verdicts are kept: pull-up classification without its event list, run ladders
// reduced to identity and quality code (all the project window reads from them).
function slimPullup(p){const pc=p&&p.pullup_classification;if(pc&&Array.isArray(pc.events)){const {events,...rest}=pc;p.pullup_classification={...rest,event_count:events.length}}return p}
function slimAnalysis(a){if(a&&Array.isArray(a.channels))for(const c of a.channels)for(const p of c.peaks||[])slimPullup(p);return a}
const LADDER_KEEP=['ladder_id','identity','status','reasons','quality_code','quality_code_source','provenance'];
function slimReport(r){if(!r)return r;for(const p of r.peaks||[])slimPullup(p);
 const q=r.quality;if(q&&Array.isArray(q.run_ladders))q.run_ladders=q.run_ladders.map(l=>Object.fromEntries(LADDER_KEEP.filter(k=>k in l).map(k=>[k,l[k]])));return r}
function compactAnalysis(a){const out={};for(const k of ['source_sha256','analysis_version','peak_settings','sizing','standard_definition_context','pullup_detection'])
 if(a[k]!==undefined)out[k]=a[k];// Pull-up classification keeps its verdict; its per-attempt event trace (several KB per peak, unused after analysis) is dropped,
 // otherwise a run cohort (sample + every ladder of the folder) grows to tens of MB per genotyping request.
 const slim=p=>{const pc=p.pullup_classification;if(!pc||!pc.events)return p;const {events,...rest}=pc;return {...p,pullup_classification:{...rest,event_count:events.length}}};
 out.channels=a.channels.map(c=>({tag:c.tag,group:c.group,peaks:(c.peaks||[]).map(slim)}));return out}

// ---------- quality mapping ----------
const ranges={pass:.75,low:.25};
const cat=v=>v==null?'na':v>=ranges.pass?'pass':v<=ranges.low?'low':'check';
const code=c=>c==null?'na':c===0?'pass':c===1?'check':c===2?'low':'na';
function samplePqv(s){
 const q={SOS:'na',SQ:'na',SSPK:'na',MIX:'na',OMR:'na',CGQ:'na'};
 const a=s.analysis;if(!a||s.status==='unanalyzed')return q;
 const sz=a.sizing||{};
 q.SQ=sz.sq_override?'override':sz.status==='accepted'?cat((sz.post_sizing_quality||sz.base_quality||{}).score??1):sz.status?'low':'na';
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
  return {sample:s,marker:m.name,channel:m.channel,calls,gq:score,gqCat:g?code(g.category?.code):'na',status:m.status}})}

// ---------- tabs ----------
const TABS=[['samples','samples'],['summary','analysisSummary'],['genotypes','genotypes'],['info','tabInfo'],['raw','tabRaw'],['ept','tabEpt']];
function renderTabs(){const t=$('tabs');t.replaceChildren();
 for(const [id,key] of TABS){const b=el('div',{class:'tab'+(state.tab===id?' on':''),role:'tab',text:T(key)});b.addEventListener('mousedown',()=>showTab(id));t.append(b)}
 for(const [id] of TABS)$('page-'+id).hidden=state.tab!==id}
function showTab(id){const was=state.tab;state.tab=id;renderTabs();render();if((was==='genotypes')!==(id==='genotypes')&&(id==='samples'||id==='genotypes'))refreshPlots()}

// ---------- navigator ----------
function currentKit(){const sel=state.project.samples.find(s=>state.selected.has(s.id)&&sampleKit(s));
 return sel?sampleKit(sel):(state.project.samples.map(sampleKit).find(Boolean)||null)}
function renderNavigator(){
 const nav=$('navigator');nav.classList.toggle('hidden',!state.showNavigator);nav.replaceChildren();
 const root=el('ul',{class:'tree root'});const li=el('li');
 const pnode=el('span',{class:'tnode'+(state.filter===null?' sel':'')});pnode.innerHTML=icon('project');pnode.append(document.createTextNode(T('project')));
 pnode.addEventListener('click',()=>{state.filter=null;buildChrome();render()});li.append(pnode);
 const ul=el('ul',{class:'tree'});
 if(state.tab==='genotypes'){
  for(const m of (currentKit()?.markers||[])){const n=el('span',{class:'tnode'});n.innerHTML=icon('marker');n.append(document.createTextNode(m.name));ul.append(el('li',{},n))}
 }else for(const rf of state.project.runFolders){
  const f=el('li');const fn=el('span',{class:'tnode'+(state.filter?.run===rf?' sel':'')});fn.innerHTML=icon('folder');fn.append(document.createTextNode(rf));
  fn.addEventListener('click',()=>{state.filter={run:rf};buildChrome();render()});f.append(fn);
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
// ---------- table cells (columns chosen by the active Table Setting) ----------
const PQV_NUM={pass:0,check:1,low:2,na:-2,override:0.5};
// MNF (Matrix Not Found): only 310 Genetic Analyzer data need a matrix; a matrix stored in the sample file is used when
// the Matrix column is empty. Newer instruments save dye-separated data (N/A).
function mnfFlag(s){const i=s.inst;if(!i||!i.is310)return 'na';if(s.matrix)return Matrices.get(s.matrix)?'pass':'low';return i.embedded?'pass':'low'}
// Sample Name (ID-X): the name stored in the sample file (SpNm/SMPL), not the file name; samples named after their file by
// earlier versions are renamed once (a name the user typed is kept).
const defaultName=f=>String(f).replace(/\.(fsa|ab1|hid)$/i,'');
async function sniffSamples(list){let any=false,renamed=false;
 for(const s of list){if(!s.blob||(s.inst&&s.inst.sampleName!==undefined))continue;
  const i=await Matrices.sniff(s.blob)||{is310:false,embedded:false,model:'',sampleName:''};s.inst={...(s.inst||{}),...i};any=true;
  if(i.sampleName&&s.name===defaultName(s.file)&&s.name!==i.sampleName){s.name=i.sampleName;renamed=true}
  // Sample type is detected in layers: file name first (at add time), then the sample name stored in the file (SpNm/SMPL),
  // then the file comment (CMNT). Only for samples whose type was chosen automatically and not changed by the user since.
  if(s.typeAuto&&s.type==='sample'){const t=[i.sampleName,i.comment].map(x=>x?sampleTypeFromName(x):'sample').find(x=>x!=='sample');if(t){s.type=t;renamed=true}}}
 if(renamed)state.project.dirty=true;if(any)render()}
function arnmCodes(s){const r=[];
 if(!s.blob&&s.method!==EXAMPLE_METHOD)r.push('arsSfnf');
 if(mnfFlag(s)==='low')r.push('arsMnf'); // ID-X SFNF: the .fsa is not available (re-associate with Edit > Define New Sample File Path)
 if(s.method!==EXAMPLE_METHOD&&!Methods.get(s.method))r.push('arnmMethod');
 if(!SIZE_STANDARDS[s.sizeStd]&&s.method!==EXAMPLE_METHOD)r.push('arnmStandard');
 if(!s.panelKey)r.push('arnmPanel');
 if(s.type!=='ladder'&&s.panelKey&&!state.project.samples.some(x=>x.runFolder===s.runFolder&&x.type==='ladder'&&x.panelKey===s.panelKey))r.push('arnmLadder');
 return r}
const arnmReasons=s=>arnmCodes(s).map(k=>T(k));
// Requirements without which a sample cannot be analyzed at all (ID-X: non-optional requirements).
const BLOCKING=new Set(['arsSfnf','arsMnf','arnmMethod','arnmStandard']);
const blocked=s=>arnmCodes(s).some(k=>BLOCKING.has(k));
// MIX PQV (ID-X help): two or more markers with three or more alleles, or one such marker and two or more
// markers with a PHR flag, or any edited marker -> Check; ladders and samples without labels -> NA.
function mixFlag(s,eff){if(s.type==='ladder'||!eff||!eff.calls.length)return 'na';
 const per={};for(const c of eff.calls)if(c.label)per[c.marker]=(per[c.marker]||0)+1;
 const many=Object.values(per).filter(n=>n>=3).length;const phr=genoRows(s).filter(r=>r.flags.PHR==='check').length;
 return many>=2||(many>=1&&phr>=2)||activeEdits(s).length?'check':'pass'}
// ACC (Amelogenin Cross Check, help topic "ACC"): one flag for AMEL and every Y marker of the panel.
const isAmel=n=>/^(AMEL|Amelogenin)$/i.test(n);
function accFlag(s,eff){if(!eff||s.type==='ladder'||s.type==='negative')return 'na';const kit=sampleKit(s);if(!kit)return 'na';
 const amel=kit.markers.find(m=>isAmel(m.name)),ys=kit.markers.filter(m=>m.y_marker&&!isAmel(m.name));if(!amel||!ys.length)return 'na';
 const names=new Set([amel.name,...ys.map(m=>m.name)]);if([...names].some(n=>eff.edited.has(n)))return 'na';
 const amelLabels=eff.calls.filter(c=>c.marker===amel.name&&c.label).map(c=>String(c.label).toUpperCase()).sort().join(',');
 const yHas=ys.map(m=>eff.calls.some(c=>c.marker===m.name&&c.label));
 if(amelLabels==='X,Y')return yHas.every(Boolean)?'pass':'check';
 if(amelLabels==='X'||amelLabels==='X,X')return yHas.some(Boolean)?'check':'pass';
 return 'check'}
// OMR (Outside Marker Range): peaks between two marker size ranges of the same dye whose height ratio to the highest
// peak of the dye exceeds the tetranucleotide Global Cut-off of the analysis method (1 = no filtering).
function omrPeaks(s){const rep=s.report;const kit=sampleKit(s);if(!rep||!kit||s.type==='ladder')return [];
 const cut=Number(Methods.get(s.method)?.allele?.repeats?.['4']?.cutoff??0);const peaks=(rep.peaks||[]).filter(p=>p.size_bp!=null&&!p.spike);
 const out=[];const byCh={};for(const m of kit.markers)(byCh[m.channel]=byCh[m.channel]||[]).push(m);
 for(const [ch,ms] of Object.entries(byCh)){ms.sort((a,b)=>a.min_bp-b.min_bp);const inCh=peaks.filter(p=>p.channel===ch);if(!inCh.length)continue;
  const top=Math.max(...inCh.map(p=>p.height));
  for(const p of inCh){const inside=ms.some(m=>p.size_bp>=m.min_bp&&p.size_bp<=m.max_bp);if(inside)continue;
   const between=p.size_bp>ms[0].max_bp&&p.size_bp<ms[ms.length-1].min_bp;if(!between)continue;
   if(cut<1&&p.height/top<=cut)continue;out.push({channel:ch,size_bp:p.size_bp,height:p.height,ratio:p.height/top})}}
 return out}
// SPK artifact labels (assigned automatically by the software, help topic "Artifact")
function spikeArtifacts(s){const kit=sampleKit(s);if(!kit)return [];const ch=new Set(kit.markers.map(m=>m.channel));
 return (s.report.peaks||[]).filter(p=>p.spike&&p.size_bp!=null&&ch.has(p.channel)).map(p=>({channel:p.channel,size_bp:p.size_bp,height:p.height}))}
function omrFlag(s){if(s.type==='ladder'||!s.report||!sampleKit(s))return 'na';if(activeEdits(s).some(e=>e.action!=='override_cgq'&&e.action!=='pli'))return 'check';return omrPeaks(s).length?'check':'pass'}
function sampleFlags(s){const q=samplePqv(s);
 const eff=s.report?effective(s):null;if(eff?.cgqOverride)q.CGQ='override';if(s.report&&q.SQ!=='low'){q.MIX=mixFlag(s,eff);q.OMR=omrFlag(s)}
 return {...q,SE:s.report?(activeEdits(s).length?'check':'pass'):'na',SQO:s.analysis?.sizing?.sq_override?'check':'na',MNF:mnfFlag(s),SFNF:(s.blob||s.method===EXAMPLE_METHOD)?'pass':'low',SNF:(SIZE_STANDARDS[s.sizeStd]||s.method===EXAMPLE_METHOD)?'pass':'low',
  ARNM:arnmReasons(s).length?'low':'pass'}}
const changed=s=>{if(s.status==='analyzed')s.status='changed';state.project.dirty=true;render()};
// ID-X: a value picked in a drop-down applies to every selected row when the edited row is part of the selection.
const peers=s=>state.selected.has(s.id)&&state.selected.size>1?state.project.samples.filter(x=>state.selected.has(x.id)):[s];
const changedAll=list=>{for(const x of list)if(x.status==='analyzed')x.status='changed';state.project.dirty=true;render()};
// Inline text cell (Sample ID, Comment, Profile ID, UD1-UD9): edits mark the project dirty.
function setField(s,key,v){if(!Sec.can('UPDATE_PROJECT')){Sec.denied();render();return}if(key.startsWith('ud.')){s.ud=s.ud||{};s.ud[key.slice(3)]=v}else s[key]=v;state.project.dirty=true}
function textEdit(s,key,value){const i=el('input',{type:'text',value:String(value),'data-field':key});
 i.addEventListener('mousedown',e=>e.stopPropagation());i.addEventListener('focus',()=>{state.lastField=key;state.lastCol=null});
 i.addEventListener('change',()=>{setField(s,key,i.value);renderTitle()});return {value,node:i}}
// Returns {value, node?, cls?, title?} for one Samples-table cell.
function sampleCell(id,s,flags){
 const info=s.analysis?.run_info||{};
 switch(id){
  case 'status':return {value:s.status,html:statusIcon(s),cls:'ic',title:T(s.status==='analyzed'?'stAnalyzed':s.status==='changed'?'stChanged':'stUnanalyzed')};
  case 'sampleFile':return {value:s.file};
  case 'sampleName':return {value:s.name};
  case 'sampleID':return textEdit(s,'sampleId',s.sampleId||'');
  case 'comments':return textEdit(s,'comment',s.comment??info.comment??'');
  case 'sampleType':{const sel=el('select');for(const k of ['sample','ladder','positive','negative'])sel.append(new Option(T(TYPE_KEYS[k]),k));sel.value=s.type;
   sel.addEventListener('change',()=>{const l=peers(s);for(const x of l){x.type=sel.value;x.typeAuto=false}changedAll(l)});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:T(TYPE_KEYS[s.type]),node:sel}}
  case 'analysisMethod':{const sel=methodSelect(s.method,v=>{const l=peers(s);for(const x of l)x.method=v;try{localStorage.setItem('opentrace.lastMethod',v)}catch(e){}changedAll(l)});
   return {value:s.method,node:sel,dbl:()=>{if(s.method!==EXAMPLE_METHOD)editMethod(s.method)}}}
  case 'panel':{const sel=panelSelect(s.panelKey,v=>{const l=peers(s);for(const x of l){x.panelKey=v;x.panel=v?v.split('\u0001')[1]:'';if(v)x.sizeStd=fitStandard(v,x.sizeStd)}changedAll(l)});
   return {value:s.panel||'',node:sel}}
  case 'sizeStandard':{const sel=el('select');for(const k of Object.keys(SIZE_STANDARDS))sel.append(new Option(k,k));sel.value=s.sizeStd;
   sel.addEventListener('change',()=>{const l=peers(s);for(const x of l)x.sizeStd=sel.value;changedAll(l)});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:s.sizeStd,node:sel,dbl:()=>editSizeStandard(s.sizeStd)}}
  case 'matrix':{if(!s.inst?.is310)return {value:''};const sel=el('select');sel.append(new Option('',''));
   for(const n of Matrices.names())sel.append(new Option(n,n));if(s.matrix&&!Matrices.get(s.matrix))sel.append(new Option(s.matrix,s.matrix));sel.value=s.matrix||'';
   sel.addEventListener('change',()=>{const l=peers(s).filter(x=>x===s||x.inst?.is310);for(const x of l)x.matrix=sel.value;changedAll(l)});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:s.matrix||'',node:sel}}
  case 'runName':return {value:info.run_name||''};
  case 'instrumentType':return {value:info.instrument_type||''};
  case 'instrumentID':return {value:info.instrument_id||''};
  case 'runDate':return {value:info.run_date||''};
  case 'plate':return {value:info.plate||''};
  case 'well':return {value:info.well||''};
  case 'lane':return {value:info.capillary??'',cls:'num'};
  case 'sampleFileNorm':return {value:s.analysis?.normalization?.sample_file||''};
  case 'ssNormFactor':{const n=s.analysis?.normalization;return {value:n?(n.sample_file==='Yes'&&n.factor!=null?String(n.factor):n.sample_file==='N/A'?'':'N/A'):'',cls:'num'}}
  case 'customControl':{const sel=el('select');sel.append(new Option('',''));for(const p of Profiles.customControls())sel.append(new Option(p.id,p.id));
   if(s.customControl&&!Profiles.getProfile(s.customControl))sel.append(new Option(s.customControl,s.customControl));sel.value=s.customControl||'';
   sel.addEventListener('change',()=>{const l=peers(s);for(const x of l)x.customControl=sel.value;changedAll(l)});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:s.customControl||'',node:sel}}
  case 'specimenCategory':{const sel=el('select');const types=Profiles.specimenTypes();for(const v of types)sel.append(new Option(v,v));
   const cur=s.specimenCategory||'No Export';if(!types.includes(cur))sel.append(new Option(cur,cur));sel.value=cur;
   sel.addEventListener('change',()=>{s.specimenCategory=sel.value;state.project.dirty=true;render()});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:cur,node:sel}}
  case 'profileId':return textEdit(s,'profileId',s.profileId||'');
  default:
   if(/^UD\d$/.test(id))return textEdit(s,'ud.'+id,(s.ud||{})[id]||'');
   const v=flags[id]??'na';const out={value:v,html:pqv(v),cls:'ic'};
   if(id==='SQ'&&ProjectOptions.get().metrics==='numbers'&&v!=='na'&&v!=='override'){const q=sqScore(s);if(q!=null)Object.assign(out,{html:null,value:Number(q.toFixed(3)),cls:'num'})}
   if(id==='ARNM'&&v==='low')out.title=arnmReasons(s).join('; ');return out}}
// ---------- label edits (overlay on the machine result; the report itself is never changed) ----------
const activeEdits=s=>(s.edits||[]).filter(e=>!e.undone);
const MEC_ACTIONS=new Set(['delete','to_artifact','add_artifact','rename_artifact','delete_artifact']);
function effective(s){
 const rep=s.report;const calls=(rep?.calls||[]).map(c=>({...c}));const byId=new Map(calls.map(c=>[c.id,c]));
 const out={calls,artifacts:[],edited:new Set(),gqOverride:new Set(),cgqOverride:false,pli:new Set(),reasons:{},mec:{}};const arts=new Map();
 for(const e of activeEdits(s)){
  if(e.action==='override_cgq'){out.cgqOverride=true;continue}
  if(e.action==='pli'){if(e.new==='on')out.pli.add(e.marker);else out.pli.delete(e.marker);continue}
  // ID-X: edits that yield an artifact label (delete, allele->artifact, artifact->artifact) go to MEC, allele label changes to AE Reason For Change
  out.edited.add(e.marker);if(MEC_ACTIONS.has(e.action))out.mec[e.marker]=e.reason;else if(e.action!=='override_gq')out.reasons[e.marker]=e.reason;
  if(e.action==='override_gq')out.gqOverride.add(e.marker);
  else if(e.action==='rename'){const c=byId.get(e.callId);if(c){c.label=e.new;c.user_edited=true}}
  else if(e.action==='delete'){const i=calls.findIndex(c=>c.id===e.callId);if(i>=0)calls.splice(i,1)}
  else if(e.action==='add'){const c={...e.peak,id:'u'+e.id,marker:e.marker,label:e.new,flags:['user_added'],user_edited:true};calls.push(c);byId.set(c.id,c)}
  else if(e.action==='move'){const c=byId.get(e.callId);if(c){Object.assign(c,e.peak,{label:e.new,user_edited:true})}}
  else if(e.action==='to_artifact'){const i=calls.findIndex(c=>c.id===e.callId);if(i>=0){const c=calls.splice(i,1)[0];
   arts.set('a'+e.id,{id:'a'+e.id,channel:c.channel,size_bp:c.size_bp,height:c.height,scan:c.scan,marker:c.marker,label:e.new,from:c.id,orig:c.label})}}
  else if(e.action==='add_artifact'){arts.set('a'+e.id,{id:'a'+e.id,...e.peak,marker:e.marker,label:e.new})}
  else if(e.action==='rename_artifact'){const a=arts.get(e.artifactId);if(a)a.label=e.new}
  else if(e.action==='delete_artifact'){arts.delete(e.artifactId)}
  else if(e.action==='artifact_to_allele'){const a=arts.get(e.artifactId);if(a){arts.delete(e.artifactId);
   const c={channel:a.channel,size_bp:a.size_bp,height:a.height,scan:a.scan,id:'u'+e.id,marker:e.marker,label:e.new,flags:['user_added'],user_edited:true};calls.push(c);byId.set(c.id,c)}}}
 out.artifacts=[...arts.values()];
 calls.sort((a,b)=>a.size_bp-b.size_bp);return out}
// Audit items for one label edit (map Allele / Sample of the ID-X audit map).
function editItems(sampleId,spec){const s=state.project.samples.find(x=>x.id===sampleId);if(!s||!s.report)return [];
 const eff=effective(s);const old=spec.callId?eff.calls.find(c=>c.id===spec.callId):spec.artifactId?eff.artifacts.find(a=>a.id===spec.artifactId):null;const inst=`${s.runFolder}/${s.file}`;
 const size=old?.size_bp??spec.peak?.size_bp;const where=`${inst} · ${spec.marker||''}${size!=null?' · '+Number(size).toFixed(2)+' bp':''}`;
 if(spec.action==='override_gq')return [{map:'Sample',attribute:'OverrideGQ',instance:`${inst} · ${spec.marker}`,old:'',new:'1.0',label:`${T('overrideGq')} (${spec.marker})`,event:'Override GQ'}];
 if(spec.action==='override_cgq')return [{map:'Sample',attribute:'OverrideCGQ',instance:inst,old:'',new:'1.0',label:T('overrideCgq'),event:'Override CGQ'}];
 if(spec.action==='pli')return [{map:'Sample',attribute:'modified',instance:`${inst} · ${spec.marker}`,old:spec.new==='on'?'':'PLI',new:spec.new==='on'?'PLI':'',label:`PLI (${spec.marker})`,event:'Partial Locus Indicator'}];
 const attr={move:'modified',add:'created',rename:'modified',delete:'deleted',to_artifact:'modified',add_artifact:'created',rename_artifact:'modified',delete_artifact:'deleted',artifact_to_allele:'modified'}[spec.action];
 const gone=spec.action==='delete'||spec.action==='delete_artifact';
 return [{map:'Allele',attribute:attr,instance:where,old:old?String(old.label):'',new:gone?'':String(spec.new??''),label:`${T('colAllele')} ${spec.marker||''}`,
  event:spec.action==='move'?'Move Allele':{created:'Add Allele',modified:'Modify Allele',deleted:'Delete Allele'}[attr]}]}
// spec: {action:'rename'|'delete'|'add'|'override_gq'|'override_cgq', marker, callId?, peak?, new?, reason?}
// reason === undefined -> ask here (Reason for Change when the audit state is On); a string -> already asked by the caller.
async function applyEdit(sampleId,spec){
 const s=state.project.samples.find(x=>x.id===sampleId);if(!s||!s.report)return false;
 if(!Sec.can('UPDATE_PROJECT')){Sec.denied();return false}
 let label=spec.new;
 if(spec.reason===undefined&&(spec.action==='rename'||spec.action==='add')){label=await promptLabel(spec.action==='add'?T('addAlleleLabel'):T('renameAllele'),spec.new||'');if(label==null)return false}
 const items=editItems(sampleId,{...spec,new:label});let reason=spec.reason;
 if(reason===undefined){const ch=await Sec.change({items,title:T('reasonTitle')});if(!ch)return false;reason=ch.reason}
 if(typeof reason!=='string'||reason.length>500)return false;
 pushEdit(s,spec,label,reason);state.project.dirty=true;render();
 await Sec.record(items.map(x=>({...x,reason})));return true}
function pushEdit(s,spec,label,reason){
 const user=Sec.user?.name||'';
 const eff=effective(s);const old=spec.callId?eff.calls.find(c=>c.id===spec.callId):spec.artifactId?eff.artifacts.find(a=>a.id===spec.artifactId):null;
 s.edits=s.edits||[];const id=(s.edits.reduce((m,e)=>Math.max(m,e.id),0)||0)+1;
 s.edits.push({id,at:new Date().toISOString(),user,reason,action:spec.action,marker:spec.marker||null,callId:spec.callId||null,artifactId:spec.artifactId||null,
  peak:spec.peak||null,old:old?old.label:null,new:(spec.action==='delete'||spec.action==='delete_artifact')?null:label??null,size_bp:old?.size_bp??spec.peak?.size_bp??null});
 s.rev=(s.rev||0)+1}
// Several label edits with one reason that the caller has already asked for (plot window: Delete Selected Labels,
// Delete Multiple Labels, one artifact label for several peaks): applied together, the project table redrawn once.
// list: [{id, spec}] with spec.new already final. Returns the number applied.
async function applyEdits(list,reason){
 if(!Sec.can('UPDATE_PROJECT')){Sec.denied();return 0}
 if(typeof reason!=='string'||reason.length>500)return 0;
 const recs=[];let n=0;
 for(const {id,spec} of list){const s=state.project.samples.find(x=>x.id===id);if(!s||!s.report)continue;
  const items=editItems(id,spec);pushEdit(s,spec,spec.new,reason);recs.push(...items.map(x=>({...x,reason})));n++}
 if(n){state.project.dirty=true;render();await Sec.record(recs)}
 return n}
function promptLabel(title,value){return new Promise(res=>{dialog(title,b=>b.append(el('div',{class:'field'},el('label',{text:T('colAllele')}),el('input',{type:'text',id:'edLabel',value}))),
 [[T('ok'),b=>{const v=b.querySelector('#edLabel').value.trim();if(!v||v.length>40){message(T('badBinName'));return false}res(v);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
function labelEditViewer(){
 const rows=[];for(const s of state.project.samples)for(const e of s.edits||[])rows.push({s,e});
 rows.sort((a,b)=>a.e.at<b.e.at?1:-1);let sel=null;
 const d=dialog(T('labelEditViewer'),()=>{},[[T('undoEdit'),()=>{if(!sel)return message(T('selectRow')),false;if(sel.e.undone)return false;
    sel.e.undone=true;sel.e.undoneAt=new Date().toISOString();sel.s.rev=(sel.s.rev||0)+1;state.project.dirty=true;render();draw();return false}],
  [T('exportTable'),()=>{const head=['Sample File','Sample Name','Marker','Size','Old Label','New Label','Action','Reason For Change','User','Date','Undone'];
    const text=[head,...rows.map(({s,e})=>[s.file,s.name,e.marker||'',e.size_bp!=null?e.size_bp.toFixed(2):'',e.old??'',e.new??'',e.action,e.reason,e.user,e.at,e.undone?'yes':''])]
     .map(r=>r.join('\t')).join('\r\n')+'\r\n';Methods.download(`${state.project.name||'project'}_label_edits.txt`,text,'text/plain');return false}],[T('closeWin'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('levdlg');
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('colSampleFile'),T('colMarker'),T('colSize'),T('levOld'),T('levNew'),T('levAction'),T('reasonForChange').replace(':',''),T('userName').replace(':',''),T('levDate')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');for(const r of rows){const e=r.e;const tr=el('tr',{class:(sel===r?'sel ':'')+(e.undone?'undone':'')},
   el('td',{text:r.s.file}),el('td',{text:e.marker||''}),el('td',{class:'num',text:e.size_bp!=null?e.size_bp.toFixed(2):''}),el('td',{text:e.old??''}),el('td',{text:e.new??''}),
   el('td',{text:T('act_'+e.action)+(e.undone?' ('+T('undone')+')':'')}),el('td',{text:e.reason}),el('td',{text:e.user}),el('td',{text:e.at.replace('T',' ').slice(0,19)}));
   tr.addEventListener('mousedown',()=>{sel=r;draw()});tb.append(tr)}
  t.append(tb);b.append(el('div',{class:'grid mgrgrid'},t));if(!rows.length)b.append(el('div',{class:'note',text:T('noEdits')}))};
 draw()}
// Control Concordance: positive/negative controls checked against the custom control profile or the panel's control alleles.
function ccFlag(rep,marker){const row=(rep.quality?.controls?.markers||[]).find(x=>x.marker===marker);if(!row||!row.comparison_performed)return 'na';return row.cc===0?'pass':'low'}
function controlDefinition(s){const kit=sampleKit(s);if(!kit)return null;const prof=s.customControl?Profiles.getProfile(s.customControl):null;
 const markers={};for(const m of kit.markers){const exp=prof?(prof.markers[m.name]||[]):(m.control_alleles||[]);
  markers[m.name]={expected_alleles:s.type==='negative'?[]:exp,quality_sensor:!!m.internal_qc,is_y_marker:!!m.y_marker}}
 return {schema_version:1,sample_type:s.type==='negative'?'Negative Control':'Positive Control',markers}}
function genoRows(s){
 const rep=s.report;if(!rep||!rep.allele_calling)return [];
 const gq=Object.fromEntries((rep.quality?.marker_gq?.markers||[]).map(m=>[m.marker,m]));const eff=effective(s);
 const acc=accFlag(s,eff);const kitM=new Map((sampleKit(s)?.markers||[]).map(m=>[m.name,m]));const nShow=TableSettings.active().genotypes.numAlleles;
 return (rep.allele_calling.markers||[]).map(m=>{const calls=eff.calls.filter(c=>c.marker===m.name);
  const g=gq[m.name];const metrics=g?.score?.metrics||{};
  const f=k=>metrics[k]===1?'check':metrics[k]===0?'pass':'na';
  const flags={OS:f('offscale'),BIN:f('binning'),PHR:f('ratio'),LPH:f('absolute_height'),MPH:f('high_height'),SPK:f('spike'),
   AN:f('allele_number'),BD:f('broad'),OVL:f('overlap'),ACC:(isAmel(m.name)||kitM.get(m.name)?.y_marker)?acc:'na',CC:ccFlag(rep,m.name),
   ADO:calls.filter(c=>c.label).length>nShow?'check':'pass',PLI:eff.pli.has(m.name)?'check':'na',ME:eff.edited.has(m.name)?'check':'pass',
   GQ:eff.gqOverride.has(m.name)?'override':g?code(g.category?.code):'na'};
  return {sample:s,marker:m.name,channel:m.channel,calls,gq:eff.gqOverride.has(m.name)?1:g?.score?.gq,flags,peaks:rep.peaks||[],reason:eff.reasons[m.name]||'',mec:eff.mec[m.name]||''}})}
function genoCell(id,r,i){
 const s=r.sample;const gts=TableSettings.active().genotypes;let c;
 if((gts.onlyBinned||gts.limitToBins)&&r.marker){const bins=(sampleKit(s)?.markers||[]).find(m=>m.name===r.marker)?.bins||[];
  // 'Show only binned alleles in their respective bin positions': column i holds the call of bin i; 'Limit ... to the number of bins': no columns beyond the marker's bins
  if(gts.onlyBinned){const lab=bins[i]?.label;c=lab==null?undefined:r.calls.find(x=>x.label===lab)}else c=!bins.length||i<bins.length?r.calls[i]:undefined}
 else c=r.calls[i]??(i===1&&r.calls.length===1&&ProjectOptions.get().dupHomo&&autosomal(r.marker)?r.calls[0]:undefined);const info=s.analysis?.run_info||{};
 switch(id){
  case 'sampleFile':return {value:s.file};case 'sampleName':return {value:s.name};case 'sampleID':return {value:s.sampleId||''};
  case 'runName':return {value:info.run_name||''};case 'panel':return {value:s.panel||''};case 'marker':return {value:r.marker};
  case 'dye':return {value:dyeLetter(r.channel)};
  case 'allele':return {value:c?c.label:''};
  case 'size':return {value:c?Number(c.size_bp.toFixed(2)):'',cls:'num'};
  case 'height':return {value:c?Math.round(c.height):'',cls:'num'};
  case 'area':{const p=c&&r.peaks[c.peak_index];return {value:p&&p.area!=null?Math.round(p.area):'',cls:'num'}}
  case 'dataPoint':return {value:c?Math.round(c.scan):'',cls:'num'};
  case 'aeReason':return {value:r.reason||''};
  case 'MEC':return {value:r.mec||''};
  case 'PLI':return {value:r.flags.PLI==='check'?'✓':'',cls:'ic'};
  case 'profileId':return {value:s.profileId||''};
  default:
   if(/^UD\d$/.test(id))return {value:''};
   const v=r.flags[id]??'na';
   if(id==='GQ'&&ProjectOptions.get().metrics==='numbers'&&r.gq!=null&&v!=='override')return {value:Number(r.gq.toFixed(3)),cls:'num'};
   return {value:v,html:pqv(v),cls:'ic',title:id==='GQ'&&r.gq!=null?r.gq.toFixed(3):''}}}
const sqScore=s=>{const sz=s.analysis?.sizing;return sz?(sz.post_sizing_quality||sz.base_quality||{}).score??null:null};
// Duplicate homozygous alleles (Project Options): autosomal markers only.
const autosomal=name=>!/^(AMEL|DYS|DXS|Y[-_ ]?indel|Yindel|Y)/i.test(name);
const isNum=v=>typeof v==='number'||(v!==''&&v!=null&&!isNaN(Number(v)));
function compare(a,b){if(isNum(a)&&isNum(b))return Number(a)-Number(b);return String(a).localeCompare(String(b),undefined,{numeric:true})}
function sortRows(rows,keys,cell){if(!keys.length)return rows;
 return rows.map((r,i)=>({r,i})).sort((x,y)=>{for(const k of keys){const d=compare(cell(k.col,x.r).value,cell(k.col,y.r).value);if(d)return k.desc?-d:d}return x.i-y.i}).map(x=>x.r)}
function currentSort(table){const ts=TableSettings.active();return state.sort?.[table]||ts[table].sort}
function headerClick(table,col){const cur=currentSort(table);const desc=cur[0]?.col===col?!cur[0].desc:false;
 state.sort={...(state.sort||{}),[table]:[{col,desc},...cur.filter(k=>k.col!==col)].slice(0,3)};render()}
const colKey=c=>c.base+(c.index!=null?':'+c.index:'');
// Fit to Content (Ctrl+I): measure each column of the active table under auto layout and keep the widths until the next fit.
function fitToContent(){const table=state.tab==='genotypes'?'genotypes':state.tab==='samples'?'samples':null;if(!table)return;
 const t=$(table==='samples'?'samplesGrid':'genotypesGrid').querySelector('table.g');if(!t)return;
 const prev={layout:t.style.tableLayout,w:t.style.width,mw:t.style.minWidth};t.style.tableLayout='auto';t.style.width='auto';t.style.minWidth='0';
 const w={};for(const th of t.querySelectorAll('thead th[data-key]'))w[th.dataset.key]=Math.max(30,Math.ceil(th.getBoundingClientRect().width)+2);
 t.style.tableLayout=prev.layout;t.style.width=prev.w;t.style.minWidth=prev.mw;
 state.colW={...(state.colW||{}),[table]:w};render()}
function tableHead(table,cols){
 const t=el('table',{class:'g'});const cg=el('colgroup');cg.append(el('col',{style:'width:28px'}));
 const cw=state.colW?.[table]||{};for(const c of cols)cg.append(el('col',{style:`width:${cw[colKey(c)]??c.width}px`}));t.append(cg);
 const hr=el('tr');hr.append(el('th',{text:''}));const srt=currentSort(table);
 for(const c of cols){const k=srt.findIndex(x=>x.col===c.base);const th=el('th',{text:c.head+(k===0?(srt[0].desc?' ▼':' ▲'):''),title:c.title||c.head,'data-col':c.base,'data-key':colKey(c)});
  th.addEventListener('click',()=>headerClick(table,c.base));hr.append(th)}
 t.append(el('thead',{},hr));return t}
function fillCell(td,v){if(v.cls)td.className=v.cls;if(v.title)td.title=v.title;if(v.node)td.append(v.node);else if(v.html!=null)td.innerHTML=v.html;else{td.textContent=v.value==null?'':String(v.value);td.title=td.textContent}
 if(v.cls)td.classList.add(v.cls);if(v.title)td.title=v.title}
function sampleColumns(){const ts=TableSettings.active();const by=Object.fromEntries(TableSettings.CATALOG.samples.map(c=>[c.id,c]));
 return ts.samples.visible.map(id=>({...by[id],base:id,title:T('colT_'+id)!=='colT_'+id?T('colT_'+id):by[id].head}))}
function genoColumns(){const ts=TableSettings.active();const by=Object.fromEntries(TableSettings.CATALOG.genotypes.map(c=>[c.id,c]));const out=[];
 for(const id of ts.genotypes.visible){const c=by[id];const title=T('colT_'+id)!=='colT_'+id?T('colT_'+id):c.head;
  if(c.kind==='allele')for(let i=0;i<ts.genotypes.numAlleles;i++)out.push({...c,base:id,index:i,head:`${c.head} ${i+1}`,title:`${title} ${i+1}`});
  else out.push({...c,base:id,title})}
 return out}
// Table-setting column filters (Filtering column of the Table Setting Editor).
function sampleRaw(id,s,fl){if(id==='status')return s.status;if(id==='sampleType')return s.type;if(id==='specimenCategory')return s.specimenCategory||'No Export';
 if(id==='sampleFileNorm'){const v=s.analysis?.normalization?.sample_file;return v==='Yes'?'yes':v==='No'?'no':'na'}const sp=TableSettings.filterSpec('samples',id);if(sp&&sp.type==='enum')return fl[id]??'na';return String(exportSampleValue(id,s,fl)??'')}
const ALLELE_COLS=new Set(['allele','size','height','area','dataPoint']);
function genoRaw(id,r){if(id==='dye')return dyeLetter(r.channel);const sp=TableSettings.filterSpec('genotypes',id);if(sp&&sp.type==='enum')return r.flags[id]??'na';
 if(ALLELE_COLS.has(id))return r.calls.map((c,i)=>genoCell(id,r,i).value).join(' ');return String(genoCell(id,r,0).value??'')}
function tsFilter(kind,list,raw){const fs=TableSettings.activeFilters(kind);state.tsCount=state.tsCount||{};
 if(!fs.length){delete state.tsCount[kind];return list}
 const out=list.filter(x=>fs.every(([id,f])=>TableSettings.matches(kind,id,f,raw(id,x))));state.tsCount[kind]=[out.length,list.length,fs.map(([id])=>id)];return out}
function sortedSamples(){let list=visibleSamples();const flags=new Map(list.map(s=>[s.id,sampleFlags(s)]));
 list=tsFilter('samples',list,(id,s)=>sampleRaw(id,s,flags.get(s.id)));if(state.lowQualityTop)return list;
 const out=sortRows(list,currentSort('samples'),(id,s)=>sampleCell(id,s,flags.get(s.id)));
 if(state.nonConcTop&&state.nonConc?.size)return [...out.filter(s=>state.nonConc.has(s.id)),...out.filter(s=>!state.nonConc.has(s.id))];
 return out}
function sortedGenoRows(){const rows=tsFilter('genotypes',visibleSamples().flatMap(genoRows),genoRaw);return sortRows(rows,currentSort('genotypes'),(id,r)=>genoCell(id,r,0))}
function renderSamples(){
 const grid=$('samplesGrid');grid.replaceChildren();const cols=sampleColumns();const t=tableHead('samples',cols);
 const tb=el('tbody');const list=sortedSamples();
 list.forEach((s,i)=>{const flags=sampleFlags(s);
  const tr=el('tr',{class:state.selected.has(s.id)?'sel':''});tr.dataset.id=s.id;tr.append(el('td',{class:'rh',text:String(i+1)}));
  for(const c of cols){const v=sampleCell(c.base,s,flags);const td=el('td');td.dataset.col=c.base;fillCell(td,v);if(v.dbl)td.addEventListener('dblclick',e=>{e.stopPropagation();v.dbl()});tr.append(td)}
  tr.addEventListener('mousedown',e=>selectRow(e,s.id,list.map(x=>x.id)));
  tr.addEventListener('dblclick',()=>displayPlots());tb.append(tr)});
 t.append(tb);grid.append(t);
 if(!list.length)grid.append(el('div',{class:'empty',text:state.project.name?'':T('untitled')}))}
function selectRow(e,id,order){
 if(e.shiftKey&&state.anchor!=null){const a=order.indexOf(state.anchor),b=order.indexOf(id);const [lo,hi]=a<b?[a,b]:[b,a];
  state.selected=new Set(order.slice(lo,hi+1))}
 else if(e.ctrlKey||e.metaKey){state.selected.has(id)?state.selected.delete(id):state.selected.add(id);state.anchor=id}
 else{state.selected=new Set([id]);state.anchor=id}
 // selecting samples selects all their genotypes (in the order the samples were selected)
 state.genoSel=[...state.selected].flatMap(sid=>{const s=state.project.samples.find(x=>x.id===sid);return s?genoRows(s).map(r=>sid+'|'+r.marker):[]});
 render();refreshPlots()}
// Genotypes table: one row per sample and marker; the selection order is the display order in the Genotypes plot.
function selectGeno(e,key,order){
 if(e.shiftKey&&state.genoAnchor!=null){const a=order.indexOf(state.genoAnchor),b=order.indexOf(key);const [lo,hi]=a<b?[a,b]:[b,a];state.genoSel=order.slice(lo,hi+1)}
 else if(e.ctrlKey||e.metaKey){state.genoSel=state.genoSel.includes(key)?state.genoSel.filter(k=>k!==key):[...state.genoSel,key];state.genoAnchor=key}
 else{state.genoSel=[key];state.genoAnchor=key}
 state.selected=new Set(state.genoSel.map(k=>+k.split('|')[0]));render();refreshPlots()}
function refreshPlots(){if(!ProjectOptions.get().updatePlots)return;try{const w=state.plotWin;if(w&&!w.closed&&w.otRefresh)w.otRefresh()}catch(e){}}

// ---------- genotypes table ----------
function renderGenotypes(){
 const grid=$('genotypesGrid');grid.replaceChildren();const cols=genoColumns();const t=tableHead('genotypes',cols);
 const tb=el('tbody');const rows=sortedGenoRows();const order=[...new Set(rows.map(r=>r.sample.id))];
 const keys=rows.map(r=>r.sample.id+'|'+r.marker);const gs=new Set(state.genoSel);
 rows.forEach((r,n)=>{const key=keys[n];const tr=el('tr',{class:gs.has(key)?'sel':''});tr.dataset.key=key;tr.append(el('td',{class:'rh',text:String(n+1)}));
  for(const c of cols){const td=el('td');fillCell(td,genoCell(c.base,r,c.index||0));tr.append(td)}
  if(r.calls.length>TableSettings.active().genotypes.numAlleles)tr.title=T('moreAlleles');
  tr.addEventListener('mousedown',e=>selectGeno(e,key,keys));tr.addEventListener('dblclick',()=>displayPlots());tb.append(tr)});
 t.append(tb);grid.append(t)}
function dyeLetter(tag){const i={DATA1:'B',DATA2:'G',DATA3:'Y',DATA4:'R'}[tag];return i||(tag==='DATA105'?'O':tag==='DATA106'?'P':'')}

// ---------- analysis summary ----------
const worst=s=>{const v=Object.values(samplePqv(s));return v.includes('low')?'low':v.includes('check')?'check':'pass'};
const ladderClass=s=>s.ladderQuality===0&&sqPass(s)?(s.ladderUsed?'used':'unused'):'low';
function filterLink(ids,label){const a=el('a',{text:String(ids.length),'data-count':String(ids.length)});
 if(ids.length)a.addEventListener('click',()=>{state.filter={ids:new Set(ids),label};showTab('samples')});else a.classList.add('zero');return a}
const pqvTip=s=>`${s.file}: `+Object.entries(samplePqv(s)).filter(([,v])=>v!=='na').map(([k,v])=>`${k} ${v}`).join(', ');
// Ladder quality per run folder (Analysis Summary and Allelic Ladder Analysis Summary).
function ladderTable(runFolders,{group=false}={}){
 const t=el('table',{class:'ladtab'});const head=el('tr',{},el('th',{text:T('sumRunFolder')}),el('th',{text:T('sumTotal')}));
 for(const [cls,key] of [['used','sumLadUsed'],['unused','sumLadUnused'],['low','sumLadLow']]){const th=el('th');th.innerHTML=(cls==='low'?pqv('check')+pqv('low'):pqv('pass')+pqv('pass'))+' ';th.append(T(key));head.append(th)}
 t.append(head);
 for(const rf of [...new Set(runFolders)]){const l=state.project.samples.filter(s=>s.runFolder===rf&&s.type==='ladder'&&s.status!=='unanalyzed');
  const tr=el('tr',{},el('td',{text:rf}),el('td',{},filterLink(l.map(s=>s.id),rf)));
  for(const cls of ['used','unused','low']){const sub=l.filter(s=>ladderClass(s)===cls);const td=el('td',{'data-cls':cls},filterLink(sub.map(s=>s.id),rf));td.title=sub.map(pqvTip).join('\n');tr.append(td)}
  t.append(tr)}
 return t}
function renderSummary(){
 const box=$('summary');box.replaceChildren();
 const folders=state.project.runFolders;if(state.summaryRun&&!folders.includes(state.summaryRun))state.summaryRun=null;
 const all=state.project.samples.filter(s=>!state.summaryRun||s.runFolder===state.summaryRun);
 const sel=el('select',{id:'sumRun'});sel.append(new Option(T('sumAllRuns'),''));for(const f of folders)sel.append(new Option(f,f));sel.value=state.summaryRun||'';
 sel.disabled=folders.length<2;sel.addEventListener('change',()=>{state.summaryRun=sel.value||null;render()});
 box.append(el('h2',{text:T('analysisSummary')}),el('div',{class:'field sumrun'},el('label',{text:T('sumRunFolder')+':'}),sel));
 const st=el('table');st.append(el('tr',{},el('th',{text:T('sumStatus')}),el('th',{text:T('sumTotal')})));
 for(const [k,key] of [['unanalyzed','stUnanalyzed'],['analyzed','stAnalyzed'],['changed','stChanged']]){
  const ids=all.filter(s=>s.status===k).map(s=>s.id);const c=el('td');c.innerHTML=statusIcon({status:k})+' ';c.append(T(key));st.append(el('tr',{},c,el('td',{},filterLink(ids,T(key)))))}
 box.append(st,el('p',{text:T('sumIntro')}),el('p',{text:T('sumClick')}));
 const done=all.filter(s=>s.status!=='unanalyzed');
 const head=(first,cols)=>{const r=el('tr',{},el('th',{text:first}));for(const c of cols){const th=el('th');th.innerHTML=c;r.append(th)}return r};
 box.append(el('h3',{text:T('sumLadder')}),el('p',{text:T('sumLadderNote')}),ladderTable(state.summaryRun?[state.summaryRun]:folders));
 const flagRow=(label,list)=>{const ok=list.filter(s=>worst(s)==='pass'),bad=list.filter(s=>worst(s)!=='pass');
  const a=el('td',{},filterLink(ok.map(s=>s.id),label)),b=el('td',{},filterLink(bad.map(s=>s.id),label));b.title=bad.map(pqvTip).join('\n');
  return el('tr',{},el('td',{text:label}),el('td',{},filterLink(list.map(s=>s.id),label)),a,b)};
 const cols=[T('sumTotal'),pqv('pass')+' '+T('sumAllMet'),pqv('check')+pqv('low')+' '+T('sumOneNotMet')];
 box.append(el('h3',{text:T('sumControl')}));
 const ct=el('table');ct.append(head(T('sumControlType'),cols));
 const ctrl=done.filter(s=>s.type==='positive'||s.type==='negative');
 for(const [label,list] of [[T('sumTotalRow'),ctrl],[T('tPositive'),ctrl.filter(s=>s.type==='positive')],[T('tNegative'),ctrl.filter(s=>s.type==='negative')]])ct.append(flagRow(label,list));
 box.append(ct,el('h3',{text:T('sumSample')}));
 const stt=el('table');stt.append(head('',cols));stt.append(flagRow(T('samples'),done.filter(s=>s.type==='sample')));box.append(stt);
 const skipped=all.filter(s=>s.alasSkipped),refBins=all.filter(s=>s.referenceBins&&s.report);
 if(skipped.length||refBins.length){const nt=el('table');nt.append(head('',[T('sumTotal')]));
  if(skipped.length)nt.append(el('tr',{},el('td',{text:T('sumSkipped')}),el('td',{},filterLink(skipped.map(s=>s.id),T('sumSkipped')))));
  if(refBins.length)nt.append(el('tr',{},el('td',{text:T('sumRefBins')}),el('td',{},filterLink(refBins.map(s=>s.id),T('sumRefBins')))));
  box.append(el('h3',{text:T('sumGenotyping')}),nt)}}
function renderFilterBar(){const bar=$('filterBar');if(!bar)return;const f=state.filter;
 const tc=state.tsCount?.[state.tab==='genotypes'?'genotypes':'samples'];
 bar.hidden=!(f&&f.ids)&&!tc;bar.replaceChildren();if(bar.hidden)return;
 if(tc){bar.append(el('span',{class:'tsf',text:`${T('tfFilteredBy')} «${TableSettings.active().name}» (${tc[2].join(', ')}): ${tc[0]} / ${tc[1]} `}))}
 if(!(f&&f.ids))return;
 const all=el('a',{text:T('showAllSamples'),id:'clearFilter'});all.addEventListener('click',()=>{state.filter=null;render()});
 bar.append(document.createTextNode(`${T('filtered')}: ${f.label||''} (${f.ids.size}) `),all)}

// ---------- render ----------
function renderTitle(){document.title=`${T('app')}${Sec.user&&Sec.state.require_login?' ['+Sec.user.name+']':''} - ${state.project.name?(state.project.dirty?'*':'')+state.project.name:T('untitled')}`}
function render(){
 renderTitle();
 {const ts=TableSettings.active();const fz=(ts[state.tab==='genotypes'?'genotypes':'samples']||{}).fontSize;document.documentElement.style.setProperty('--tbl-font',fz?fz+'px':'')}
 renderNavigator();
 if(state.tab==='samples')renderSamples();else if(state.tab==='summary')renderSummary();else if(state.tab==='genotypes')renderGenotypes();else SampleViews.render(state.tab);
 renderFilterBar();
 const any=state.project.samples.length>0;
 for(const id of ['tbAnalyze','tbPlots','tbExport'])if($(id))$(id).disabled=!any||state.busy;
 $('stopBtn').disabled=!state.busy;
 if(!state.busy)$('statusText').textContent=state.lastMessage||`${state.project.samples.length} ${T('samples').toLowerCase()}`;
}
function progress(done,total,text){const pct=total?Math.round(100*done/total):0;$('progressBar').style.width=pct+'%';$('progressText').textContent=pct+'%';if(text)$('statusText').textContent=text}

// ---------- actions ----------
const notYet=()=>message(T('notImplemented'));
// Help (F1): hotkey reference of the project window (same bindings as the keydown handler below and the menu labels).
const HELP_KEYS=[['Ctrl+N','Новый проект','New project'],['Ctrl+O','Открыть проект','Open project'],['Ctrl+S','Сохранить проект','Save project'],
 ['Ctrl+K','Добавить образцы','Add samples'],['Ctrl+R','Анализ ожидающих образцов','Analyze pending samples'],['Ctrl+Shift+R','Анализ всех образцов','Analyze all samples'],
 ['Ctrl+L','Показать графики','Display plots'],['Ctrl+E','Экспорт таблицы','Export table'],['Ctrl+Q','Менеджер отчётов','Report Manager'],['Ctrl+P','Печать','Print'],
 ['Ctrl+J','Менеджер панелей','Panel Manager'],['Ctrl+T','Настройка таблицы','Table setting'],['Ctrl+F','Найти','Find'],['Ctrl+G','Сортировка','Sort'],
 ['Ctrl+A','Выбрать все строки','Select all rows'],['Ctrl+C','Копировать строки','Copy rows'],['Ctrl+D','Заполнить столбец вниз (Fill Down)','Fill Down'],
 ['Ctrl+I','Подогнать ширину столбцов','Fit to Content'],['Ctrl+B','Низкое качество наверх','Low quality to top'],['Ctrl+Shift+N','Навигатор','Navigator'],
 ['Ctrl+Shift+1/2/3','Вкладки Образцы / Сводка / Генотипы','Samples / Summary / Genotypes tabs'],['Ctrl+F1/F2/F3','Информация / Необработанные данные / EPT','Info / Raw data / EPT'],['F1','Эта справка','This help']];
function showHelp(){const ru=I18N.lang==='ru';
 dialog(T('helpTopics'),b=>{b.append(el('div',{class:'field',text:ru?'Горячие клавиши окна проекта (Cmd вместо Ctrl на macOS). Полный перечень ID-X — в документации проекта.':'Project window hotkeys (Cmd instead of Ctrl on macOS). The full ID-X list is in the project documentation.'}));
  const t=el('table',{class:'g'});for(const [k,a,e] of HELP_KEYS)t.append(el('tr',{},el('td',{text:k,style:'width:150px;font-family:monospace'}),el('td',{text:ru?a:e})));
  const w=el('div',{style:'max-height:360px;overflow:auto'});w.append(t);b.append(w)});}

// ---------- project library (server folder) ----------
const sid=s=>'s'+s.id;
function blobToB64(blob){return new Promise((res,rej)=>{const r=new FileReader();r.onload=()=>res(String(r.result).split(',')[1]);r.onerror=rej;r.readAsDataURL(blob)})}
function b64ToBlob(b64,name){const bin=atob(b64);const a=new Uint8Array(bin.length);for(let i=0;i<bin.length;i++)a[i]=bin.charCodeAt(i);return new File([a],name)}
function projectMeta(){const used=new Set(state.project.samples.map(s=>s.method));const kitsUsed=new Set(state.project.samples.map(s=>(s.panelKey||'').split('\u0001')[0]).filter(Boolean));
 return {comment:state.project.comment||'',security_group:state.project.securityGroup||Sec.defaultGroup(),run_folders:state.project.runFolders,bin_offsets:state.project.binOffsets||[],reports:state.project.reports||[],mixture:state.project.mixture||{},
  samples:state.project.samples.map(s=>({id:sid(s),file:s.file,name:s.name,type:s.type,runFolder:s.runFolder,method:s.method,panelKey:s.panelKey||'',panel:s.panel||'',
   sizeStd:s.sizeStd,status:s.status,sampleId:s.sampleId||'',comment:s.comment??null,profileId:s.profileId||'',specimenCategory:s.specimenCategory||'',customControl:s.customControl||'',ud:s.ud||{},origin:s.origin||'',matrix:s.matrix||'',inst:s.inst||null,ladderQuality:s.ladderQuality??null,ladderUsed:!!s.ladderUsed,alasSkipped:!!s.alasSkipped,referenceBins:!!s.referenceBins,sqOverride:s.sqOverride||null,edits:s.edits||[]})),
  snapshots:{methods:Methods.list().filter(r=>used.has(r.method.name)).map(r=>r.method),matrices:state.project.samples.map(s=>s.matrix&&Matrices.get(s.matrix)).filter((m,i,a)=>m&&a.indexOf(m)===i),sizeStds:[...new Set(state.project.samples.map(s=>s.sizeStd))].map(n=>SizeStd.get(n)).filter(Boolean),tableSetting:TableSettings.active(),
   kits:KitStore.load().kits.filter(k=>kitsUsed.has(k.name)),example:state.kit&&state.project.samples.some(s=>s.panelKey===EXAMPLE_KEY)?
    {kit:state.kit,lq:state.ladderQuality,mq:state.markerQuality}:null}}}
async function writeProject(name,{create=false}={}){
 if(state.busy)return false;if(!Sec.need('UPDATE_PROJECT'))return false;
 const isNew=create||!state.project.savedAs;
 const ch=await Sec.change({map:'Project',attribute:isNew?'created':'modified',instance:name,old:isNew?'':(state.project.savedAs||''),new:name,event:isNew?'Add Project':'Modify Project',
  esig:{group:'GM_PROJECT',action:isNew?'CREATE_PROJECT':'MODIFY_PROJECT'}});if(!ch)return false;
 state.busy=true;render();
 try{progress(0,1,T('saving'));
  const saved=await jsonPost('/api/idx-project',{action:'save_meta',name,project:projectMeta(),create,overwrite:!create});
  state.project.securityGroup=saved.security_group;state.project.owner=saved.owner;
  if(saved.opentrace_server&&saved.opentrace_server.web_lease&&WebLease.name!==name)WebLease.start(name,saved.opentrace_server);
  const list=state.project.samples;let i=0;
  for(const s of list){const need=create||state.project.savedAs!==name||s.savedRev!==s.rev;
   if(need){const payload={action:'save_sample',name,id:sid(s),data:{analysis:s.analysis||null,compact:s.compact||null,report:s.report||null}};
    if(s.blob)payload.fsa_b64=await blobToB64(s.blob);await jsonPost('/api/idx-project',payload);s.savedRev=s.rev}
   progress(++i,list.length,`${T('saving')}: ${s.file}`)}
  Object.assign(state.project,{name,savedAs:name,dirty:false});state.lastMessage=T('projectSaved')+': '+name;rememberProject(name);await ch.commit();return true}
 catch(e){message(e.message);return false}finally{state.busy=false;render()}}
function saveProject(){if(!state.project.samples.length&&!state.project.name)return;if(!Sec.need('UPDATE_PROJECT'))return;
 if(state.project.savedAs&&state.project.savedAs===state.project.name)return writeProject(state.project.name);return saveProjectAs()}
function saveProjectAs(){if(!Sec.need('UPDATE_PROJECT'))return;
 // Save As: name, comment and the security group of the project (ID-X changes a project's group only by Save As)
 const groups=Sec.groupNames().filter(g=>Sec.objectCan(g,'create'));if(!groups.length)return message(T('scNoCreateGroup'));
 const gs=el('select',{id:'spGroup'});for(const g of groups)gs.append(new Option(g,g));const cur=state.project.securityGroup||Sec.defaultGroup();gs.value=groups.includes(cur)?cur:groups[0];
 dialog(T('saveProjectAs').replace(/\.+$/,''),b=>b.append(el('div',{class:'field'},el('label',{text:T('projectName'),for:'spName'}),el('input',{type:'text',id:'spName',value:state.project.name||''})),
   el('div',{class:'field'},el('label',{text:T('projectComment')}),el('input',{type:'text',id:'spComment',value:state.project.comment||''})),
   el('div',{class:'field'},el('label',{text:T('securityGroupLbl')}),gs)),
  [[T('ok'),async b=>{const n=b.querySelector('#spName').value.trim();if(!n)return false;state.project.comment=b.querySelector('#spComment').value;
    const prevGroup=state.project.securityGroup;state.project.securityGroup=gs.value;
    const existing=(await jsonPost('/api/idx-project',{action:'list'})).projects.some(p=>p.name===n);
    if(existing&&n!==state.project.savedAs&&!(await Methods.ask(T('projectOverwrite').replace('{0}',n))))return false;
    for(const s of state.project.samples)s.savedRev=undefined;
    if(!(await writeProject(n,{create:n!==state.project.savedAs&&!existing})))state.project.securityGroup=prevGroup},true],[T('cancel'),()=>true]])}
async function confirmDiscard(){if(!state.project.dirty||!state.project.samples.length)return true;return Methods.ask(T('discardChanges'))}
async function openProject(){
 if(!Sec.need('READ_PROJECT'))return;if(!(await confirmDiscard()))return;
 let rows;try{rows=await jsonPost('/api/idx-project',{action:'list'})}catch(e){return message(e.message)}
 let selected=null;
 const d=dialog(T('openProject').replace(/\.+$/,''),()=>{},[[T('open'),async()=>{if(!selected){message(T('selectRow'));return false}await loadProject(selected);return true},true],
  [T('mgrDelete'),async()=>{if(!selected)return false;if(!Sec.need('UPDATE_PROJECT'))return false;if(await Methods.ask(T('confirmDeleteProject').replace('{0}',selected))){
    const ch=await Sec.change({map:'Project',attribute:'deleted',instance:selected,old:selected,new:'',event:'Delete Project',esig:{group:'GM_PROJECT',action:'DELETE_PROJECT'}});if(!ch)return false;
    try{await jsonPost('/api/idx-project',{action:'delete',name:selected})}catch(e){message(e.message);return false}await ch.commit();
    rows=await jsonPost('/api/idx-project',{action:'list'});selected=null;draw()}return false}],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('opendlg');
  const q=el('input',{type:'text',id:'opFilter',placeholder:T('findWhat')});
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('projectName'),T('created'),T('colLastSaved'),T('samples'),T('projectComment'),T('securityGroupLbl'),T('owner')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');t.append(tb);
  const fill=()=>{tb.replaceChildren();const f=q.value.toLowerCase();
   for(const r of rows.projects.filter(r=>r.name.toLowerCase().includes(f))){const tr=el('tr',{class:r.name===selected?'sel':''},el('td',{text:r.name}),el('td',{text:(r.created||'').replace('T',' ')}),
     el('td',{text:(r.modified||'').replace('T',' ')}),el('td',{class:'num',text:String(r.samples)}),el('td',{text:r.comment||''}),el('td',{text:r.security_group||''}),el('td',{text:r.owner||''}));
    tr.addEventListener('mousedown',()=>{selected=r.name;tb.querySelectorAll('tr').forEach(x=>x.classList.toggle('sel',x===tr))});
    tr.addEventListener('dblclick',async()=>{selected=r.name;d.close();await loadProject(r.name)});tb.append(tr)}};
  q.addEventListener('input',fill);fill();
  b.append(el('div',{class:'field'},el('label',{text:T('find')}),q),el('div',{class:'grid mgrgrid'},t),el('div',{class:'note',text:T('projectsFolder')+' '+rows.root}))};
 draw()}
async function loadProject(name){
 state.busy=true;render();
 try{progress(0,1,T('opening'));const meta=await jsonPost('/api/idx-project',{action:'open',name});
  const notes=[];const snap=meta.snapshots||{};
  for(const m of snap.methods||[])if(!Methods.get(m.name)){Methods.put(m);notes.push(T('mgrMethods')+': '+m.name)}
  for(const m of snap.matrices||[])if(!Matrices.get(m.name)){try{Matrices.put(m);notes.push(T('mgrMatrices')+': '+m.name)}catch(e){}}
  for(const m of snap.sizeStds||[])if(!SizeStd.get(m.name)){try{SizeStd.put(m);notes.push(T('mgrSizeStandards')+': '+m.name)}catch(e){}}
  if(snap.kits&&snap.kits.length){const db=KitStore.load();let added=false;for(const k of snap.kits)if(!db.kits.some(x=>x.name===k.name)){db.kits.push(k);added=true;notes.push(T('panelMgrTitle')+': '+k.name)}
   if(added){KitStore.save(db);dropCaches()}}
  if(snap.tableSetting&&!TableSettings.get(snap.tableSetting.name)){try{TableSettings.put(snap.tableSetting);notes.push(T('mgrTables')+': '+snap.tableSetting.name)}catch(e){}}
  if(snap.example){state.kit=snap.example.kit;state.ladderQuality=snap.example.lq;state.markerQuality=snap.example.mq;
   kitCache.set(EXAMPLE_KEY,{kit:snap.example.kit,lq:snap.example.lq,mq:snap.example.mq,policy:'reject'})}
  // Samples are fetched several at a time (each is several MB of JSON on real runs); order and ids as listed.
  const samples=new Array(meta.samples.length);let done=0,next=0;
  const loadOne=async idx=>{const m=meta.samples[idx];const d=await jsonPost('/api/idx-project',{action:'open_sample',name,id:m.id});const data=d.data||{};
   const id=idx+1;samples[idx]=({id,file:m.file,name:m.name,type:m.type,runFolder:m.runFolder,method:m.method,panelKey:m.panelKey,panel:m.panel,sizeStd:m.sizeStd,
    status:data.analysis?m.status:'unanalyzed',sampleId:m.sampleId,comment:m.comment,profileId:m.profileId,specimenCategory:m.specimenCategory||'',customControl:m.customControl||'',ud:m.ud||{},origin:m.origin||'',matrix:m.matrix||'',inst:m.inst||null,ladderQuality:m.ladderQuality,ladderUsed:!!m.ladderUsed,alasSkipped:!!m.alasSkipped,referenceBins:!!m.referenceBins,sqOverride:m.sqOverride||undefined,edits:m.edits||[],
    blob:d.fsa_b64?b64ToBlob(d.fsa_b64,m.file):null,analysis:slimAnalysis(data.analysis||null),compact:data.compact?slimAnalysis(data.compact):null,report:slimReport(data.report||null),rev:0,savedRev:0});
   progress(++done,meta.samples.length,`${T('opening')}: ${m.file}`)};
  await Promise.all(Array.from({length:Math.min(4,meta.samples.length)},async()=>{while(next<meta.samples.length)await loadOne(next++)}));
  state.nextId=meta.samples.length+1;
  Object.assign(state.project,{securityGroup:meta.security_group||Sec.DEFAULT_GROUP,owner:meta.owner||'',mixture:meta.mixture&&typeof meta.mixture==='object'?meta.mixture:{},reports:Array.isArray(meta.reports)?meta.reports:[],binOffsets:Array.isArray(meta.bin_offsets)?meta.bin_offsets:[],name:meta.name,savedAs:meta.name,comment:meta.comment||'',runFolders:meta.run_folders||[...new Set(samples.map(s=>s.runFolder))],samples,dirty:false});
  state.selected.clear();state.filter=null;state.sort=null;
  state.lastMessage=T('projectOpened')+': '+meta.name;sniffSamples(state.project.samples);rememberProject(meta.name);WebLease.start(meta.name,meta.opentrace_server);ensureKitsQuiet(state.project.samples);if(notes.length)message(T('projectAddedObjects')+'\n'+notes.join('\n'))}
 catch(e){message(e.message)}finally{state.busy=false;render()}}
// ---------- OpenTrace Server: one editor per project (web) ----------
// Only when the server says so (opentrace_server.web_lease): the desktop app and the stand-alone web app keep their own
// behaviour. Reading is never blocked; a project edited elsewhere opens read-only with a banner.
const WebLease={name:null,timer:null,readonly:null,
 banner(){let b=$('otWebReadOnly');if(!this.readonly||!this.name){if(b)b.remove();return}
  if(!b){b=el('div',{id:'otWebReadOnly',style:'background:#fff3cd;border-bottom:1px solid #e0b000;padding:3px 8px;font-size:12px'});document.body.insertBefore(b,document.body.firstChild?document.body.firstChild.nextSibling:null)}
  b.textContent=T('webReadOnly').replace('{0}',this.name).replace('{1}',this.readonly)},
 start(name,info){this.stop(false);if(!info||!info.web_lease)return;this.name=name;this.readonly=info.readonly||null;this.banner();
  if(!this.readonly)this.timer=setInterval(()=>this.renew(),Math.max(3000,Math.min(25000,+info.renew_ms||25000)))},
 async renew(){if(!this.name)return;let r;try{r=await jsonPost('/api/idx-project',{action:'lease_renew',name:this.name})}catch(e){return}
  if(r&&r.readonly&&!this.readonly){this.readonly=r.readonly;clearInterval(this.timer);this.timer=null;this.banner();message(T('webLeaseLost').replace('{0}',this.name).replace('{1}',r.readonly))}},
 stop(release=true){if(this.timer)clearInterval(this.timer);this.timer=null;
  if(release&&this.name&&!this.readonly){try{fetch('/api/idx-project',{method:'POST',keepalive:true,headers:{'X-Session-Token':TOKEN,'X-User-Ticket':Sec.ticket(),'X-OT-Tab':TAB,'Content-Type':'application/json'},body:JSON.stringify({action:'lease_release',name:this.name})}).catch(()=>{})}catch(e){}}
  this.name=null;this.readonly=null;this.banner()}};
window.addEventListener('pagehide',()=>WebLease.stop(true));
function rememberProject(name){try{localStorage.setItem('opentrace.lastProject',name)}catch(e){}}
async function closeProject(){if(!(await confirmDiscard()))return;WebLease.stop(true);
 Object.assign(state.project,{name:null,savedAs:null,comment:'',runFolders:[],samples:[],reports:[],binOffsets:[],dirty:false});state.selected.clear();state.lastMessage=null;render()}
// ---------- copy / fill down ----------
// Drop-down columns Fill Down can copy (value of the first selected row to the others).
const FILL_COLS={
 analysisMethod:(d,v)=>{d.method=v.method},
 panel:(d,v)=>{d.panelKey=v.panelKey;d.panel=v.panel;if(v.panelKey)d.sizeStd=fitStandard(v.panelKey,d.sizeStd)},
 sizeStandard:(d,v)=>{d.sizeStd=v.sizeStd},
 sampleType:(d,v)=>{d.type=v.type;d.typeAuto=false},
 matrix:(d,v)=>{if(d.inst?.is310)d.matrix=v.matrix},
 customControl:(d,v)=>{d.customControl=v.customControl},
 specimenCategory:(d,v)=>{d.specimenCategory=v.specimenCategory}};
// The column of the last clicked cell (any cell, including drop-downs) is the one Fill Down copies.
document.addEventListener('mousedown',e=>{const td=e.target.closest&&e.target.closest('#page-samples tbody td[data-col]');if(!td)return;
 const col=td.dataset.col;state.lastCol=FILL_COLS[col]?col:null;if(FILL_COLS[col])state.lastField=null},true);
// Project Edit > Undo/Redo: snapshots of the sample table fields taken before Fill Down / Clear / Paste.
const histU=[],histR=[];
const snapTable=()=>state.project.samples.map(s=>{const c={...s};c.ud=s.ud?{...s.ud}:s.ud;return c});
function restoreTable(snap){const by=new Map(state.project.samples.map(s=>[s.id,s]));
 for(const c of snap){const s=by.get(c.id);if(!s)continue;for(const k of Object.keys(s))if(!(k in c))delete s[k];Object.assign(s,c,{ud:c.ud?{...c.ud}:c.ud,rev:(s.rev||0)+1})}
 state.project.dirty=true;SampleViews?.invalidate?.();render();buildChrome()}
function withUndo(fn){const snap=snapTable();const r=fn();if(r instanceof Promise)return r;histU.push(snap);if(histU.length>30)histU.shift();histR.length=0;buildChrome();return r}
function undoProject(){if(!histU.length)return;histR.push(snapTable());restoreTable(histU.pop());state.project.dirty=true;buildChrome()}
window.__otProject={state,get fillDown(){return fillDown},undoProject:()=>undoProject(),redoProject:()=>redoProject()};
function redoProject(){if(!histR.length)return;histU.push(snapTable());restoreTable(histR.pop());buildChrome()}
function fillDown(){return withUndo(fillDown0)}
function fillDown0(){
 const rows=sortedSamples().filter(s=>state.selected.has(s.id));
 const col=state.lastCol;
 if(col&&FILL_COLS[col]){if(rows.length<2)return message(T('fillDownHint'));
  if(!Sec.can('UPDATE_PROJECT')){Sec.denied();return}
  const src=rows[0];const l=rows.slice(1);for(const d of l)FILL_COLS[col](d,src);changedAll(l);return}
 const key=state.lastField;if(!key)return message(T('fillDownHint'));
 if(rows.length<2)return message(T('fillDownHint'));
 const get=s=>key.startsWith('ud.')?(s.ud||{})[key.slice(3)]||'':(s[key]||'');const v=get(rows[0]);
 for(const s of rows.slice(1))setField(s,key,v);render()}
// Edit > Clear (Delete) empties the text column last clicked in the selected rows; Edit > Paste (Ctrl+V) writes the clipboard lines into that column,
// from the first selected row down (a single line goes to every selected row). Drop-down columns take a pasted value only when it names an existing choice.
function selectedRows(){return sortedSamples().filter(s=>state.selected.has(s.id))}
function clearCells(){return withUndo(clearCells0)}
function clearCells0(){const rows=selectedRows();const key=state.lastField;if(!rows.length||!key)return message(T('clearHint'));
 if(!Sec.can('UPDATE_PROJECT')){Sec.denied();return}for(const s of rows)setField(s,key,'');render()}
async function pasteCells(){const snap=snapTable();const r=await pasteCells0();histU.push(snap);if(histU.length>30)histU.shift();histR.length=0;buildChrome();return r}
async function pasteCells0(){const rows=selectedRows();const key=state.lastField,col=state.lastCol;if(!rows.length||(!key&&!col))return message(T('pasteHint'));
 let text='';try{text=await navigator.clipboard.readText()}catch(e){return message(T('pasteDenied'))}
 const lines=text.replace(/\r/g,'').split('\n');while(lines.length&&lines[lines.length-1]==='')lines.pop();if(!lines.length)return;
 if(!Sec.can('UPDATE_PROJECT')){Sec.denied();return}
 const cell=l=>String(l).split('\t')[0].trim();let n=0;
 rows.forEach((s,i)=>{const l=lines.length===1?lines[0]:lines[i];if(l==null)return;const v=cell(l);
  if(key){setField(s,key,v);n++;return}
  if(col==='sizeStandard'&&SIZE_STANDARDS[v]){s.sizeStd=v;n++}
  else if(col==='analysisMethod'&&Methods.get(v)){s.method=v;n++}
  else if(col==='panel'){const c=panelChoices().find(x=>x.label===v);if(c){FILL_COLS.panel(s,{panelKey:c.key,panel:c.panel});n++}}});
 if(col)changedAll(rows);else render();state.lastMessage=T('pasted')+': '+n;renderTitle()}
// ---------- size standards and Size Match Editor ----------
const firstSelected=()=>sortedSamples().find(s=>state.selected.has(s.id));
function editSizeStandard(nm){const name=(typeof nm==='string'&&nm)||firstSelected()?.sizeStd||SizeStd.names()[0];const s=SizeStd.get(name);if(!s)return message(T('arnmStandard'));
if(!Sec.need('READ_SIZE_STANDARD'))return;
 SizeStd.openEditor(s,{restrict:Sec.can('UPDATE_SIZE_STANDARD')?null:'read',onSave:(ns,prev)=>{SizeStd.put(ns,prev);for(const x of state.project.samples)if(x.sizeStd===prev){x.sizeStd=ns.name;if(x.status==='analyzed')x.status='changed'}
  settingsCache.clear();state.project.dirty=true;render()}})}
async function reanalyze(s){s.status='changed';s.rev=(s.rev||0)+1;const keep=state.selected;state.selected=new Set([s.id]);
 try{await analyze('selected')}finally{state.selected=keep;render()}}
function openSizeMatch(){const s=firstSelected();if(!s||!s.analysis)return message(T('selectAnalyzedSample'));
 SizeMatch.open(s,{override:async(anchors,reason)=>{if(!Sec.need('UPDATE_PROJECT'))return;s.sqOverride={anchors,reason};state.project.dirty=true;
   Sec.record([{map:'Sample',attribute:'OverrideSQ',instance:`${s.runFolder}/${s.file}`,old:'',new:`${anchors.length} anchors`,reason,event:'Override SQ'}]);await reanalyze(s)},
  clearOverride:async()=>{delete s.sqOverride;state.project.dirty=true;await reanalyze(s)}})}
window.addEventListener('opentrace-sizestd',()=>{settingsCache.clear();render()});
function newProject(){
 dialog(T('newProjectTitle'),b=>{b.append(el('div',{class:'field'},el('label',{text:T('projectName'),for:'npName'}),el('input',{type:'text',id:'npName',value:''})))},
  [[T('ok'),body=>{const v=body.querySelector('#npName').value.trim();if(!v)return false;
    Object.assign(state.project,{name:v,savedAs:null,comment:'',runFolders:[],samples:[],reports:[],binOffsets:[],dirty:true});state.selected.clear();state.filter=null;render()},true],[T('cancel'),()=>true]])}
// File-name heuristics for sample type (used when the file carries no type itself); the user can change the type in the table.
// The name is split into tokens at every non-alphanumeric character, so "AL", "L", "L2", "PC", "PC1", "NC", "NTC_03" are
// recognised as whole name parts and not as letters inside a longer word (e.g. "L" in "Sample" or "NC" in "Vance").
const SAMPLE_TYPE_TOKENS={
 ladder:/^(al|l|lad|ladder|allelic\w*|alleliclad\w*|лестница|ладдер|ал|л)\d*$/i,
 negative:/^(nc|ntc|neg|negative|negctrl|negcontrol|blank|нк|отр\w*)\d*$/i,
 positive:/^(pc|pos|positive|posctrl|poscontrol|ctrl|control|ctl|9947a?|2800m|пк|пол\w*)\d*$/i};
function sampleTypeFromName(n){
 const base=String(n).replace(/\.(fsa|ab1|hid)$/i,'');
 // Words that are unambiguous even when glued to other text ("Ladder_PG", "AllelicLadder", "negctrl").
 if(/ladder|allelic|лестниц|ладдер/i.test(base))return 'ladder';
 if(/(^|[^a-z])(neg(ative)?|ntc|blank)/i.test(base)||/(^|[^а-яё])отриц|нег/i.test(base))return 'negative';
 if(/(^|[^a-z])(pos(itive)?|control)|9947|2800m/i.test(base)||/(^|[^а-яё])полож/i.test(base))return 'positive';
 const toks=base.split(/[^A-Za-z0-9А-Яа-яЁё]+/).filter(Boolean);
 // Tokens that carry a digit prefix glued to letters (e.g. "01PC") are split into letter and digit runs as well.
 const parts=[];for(const t of toks){parts.push(t);for(const m of t.match(/[A-Za-zА-Яа-яЁё]+\d*/g)||[])if(m!==t)parts.push(m)}
 // "K+" / "К+" and "+C" style positive markers.
 if(/(^|[_\-\s.])[KkКк]\+|(^|[_\-\s.])\+[Cc]($|[_\-\s.])/.test(base))return 'positive';
 // "007" is the AmpFlSTR control DNA only as the first name part (not a well/plate index like _007_D07).
 if(/^007([^0-9]|$)/.test(base))return 'positive';
 for(const k of ['ladder','negative','positive'])if(parts.some(t=>SAMPLE_TYPE_TOKENS[k].test(t)))return k;
 return 'sample'}
function addSamples(){
 if(!Sec.need('UPDATE_PROJECT'))return;
 if(!state.project.name)Object.assign(state.project,{name:T('untitled'),dirty:true});
 let files=[];
 dialog(T('addTitle'),b=>{
  const list=el('div',{class:'filelist'});const input=el('input',{type:'file',multiple:'',accept:'.fsa,.ab1,.hid',style:'display:none'});
  const sel=new Set();let anchor=-1;
  const redraw=()=>{for(const f of [...sel])if(!files.includes(f))sel.delete(f);
   list.replaceChildren(...files.map((f,i)=>{const d=el('div',{text:f.webkitRelativePath||f.name,class:sel.has(f)?'sel':''});
    d.addEventListener('click',e=>{if(e.shiftKey&&anchor>=0){sel.clear();for(let k=Math.min(anchor,i);k<=Math.max(anchor,i);k++)sel.add(files[k])}
     else if(e.ctrlKey||e.metaKey){sel.has(f)?sel.delete(f):sel.add(f);anchor=i}else{sel.clear();sel.add(f);anchor=i}redraw()});return d}));
   cnt.textContent=files.length?T('addCount').replace('{0}',files.length):''};
  const addFiles=fl=>{const seen=new Set(files.map(f=>(f.webkitRelativePath||f.name)+'|'+f.size));
   for(const f of fl){if(!/\.(fsa|ab1|hid)$/i.test(f.name))continue;const k=(f.webkitRelativePath||f.name)+'|'+f.size;if(!seen.has(k)){seen.add(k);files.push(f)}}redraw()};
  const cnt=el('div',{class:'note'});
  list.tabIndex=0;list.addEventListener('keydown',e=>{const k=e.code;
   if((e.ctrlKey||e.metaKey)&&k==='KeyA'){e.preventDefault();files.forEach(f=>sel.add(f));redraw()}
   else if(k==='Delete'||k==='Backspace'){e.preventDefault();files=files.filter(f=>!sel.has(f));sel.clear();redraw()}});
  list.addEventListener('dragover',e=>e.preventDefault());list.addEventListener('drop',e=>{e.preventDefault();addFiles([...(e.dataTransfer?.files||[])])});
  input.addEventListener('change',()=>{addFiles(input.files);input.value=''});
  const choose=el('button',{class:'btn',text:T('addChoose'),type:'button'});choose.addEventListener('click',()=>input.click());
  const dirIn=el('input',{type:'file',webkitdirectory:'',directory:'',multiple:'',style:'display:none'});
  dirIn.addEventListener('change',()=>{addFiles(dirIn.files);dirIn.value=''});
  const chooseDir=el('button',{class:'btn',text:T('addChooseFolder'),type:'button'});chooseDir.addEventListener('click',()=>dirIn.click());
  const clear=el('button',{class:'btn',text:T('addClear'),type:'button'});clear.addEventListener('click',()=>{if(sel.size){files=files.filter(f=>!sel.has(f));sel.clear()}else files=[];redraw()});
  const ss=el('select',{id:'addStd'});for(const k of Object.keys(SIZE_STANDARDS))ss.append(new Option(k,k));
  const od=ProjectOptions.get().add;if(od.sizeStd&&SIZE_STANDARDS[od.sizeStd])ss.value=od.sizeStd;
  let lastKey='';try{lastKey=od.panelKey||localStorage.getItem('opentrace.lastPanel')||''}catch(e){}
  if(!panelChoices().some(c=>c.key===lastKey))lastKey='';
  const ps=panelSelect(lastKey,v=>{if(v)ss.value=fitStandard(v,ss.value)});ps.id='addPanel';if(lastKey)ss.value=fitStandard(lastKey,ss.value);
  const fs=el('fieldset',{},el('legend',{text:T('addSettings')}),
   el('div',{class:'field'},el('label',{text:T('runFolderName')}),el('input',{type:'text',id:'addRun',value:'Run '+(state.project.runFolders.length+1)})),
   el('div',{class:'field'},el('label',{text:T('colAnalysisMethod')}),(()=>{const dm=ProjectOptions.get().add.method;const m=methodSelect(dm&&Methods.get(dm)?dm:defaultMethod(),v=>{m.value=v});m.id='addMethod';return m})()),
   el('div',{class:'field'},el('label',{text:T('colPanel')}),ps),
   el('div',{class:'field'},el('label',{text:T('colSizeStandard')}),ss));
  if(!panelChoices().length)fs.append(el('div',{class:'note',text:T('noPanelsYet')}));
  b.append(el('div',{text:T('addFiles')}),list,cnt,el('div',{style:'display:flex;gap:6px;margin:6px 0'},choose,chooseDir,clear,input,dirIn),fs)},
  [[T('add'),body=>doAdd(body,false),true],[T('addAndAnalyze'),body=>doAdd(body,true)],[T('cancel'),()=>true]])
 function doAdd(body,andAnalyze){{if(!files.length)return false;const run=body.querySelector('#addRun').value.trim()||'Run';const std=body.querySelector('#addStd').value;const pkey=body.querySelector('#addPanel').value;const meth=body.querySelector('#addMethod').value;
    try{localStorage.setItem('opentrace.lastMethod',meth)}catch(e){}
    try{localStorage.setItem('opentrace.lastPanel',pkey)}catch(e){}
    addSampleFiles(files,run,meth,pkey,std);if(andAnalyze)analyze('pending')}return true}}
function addSampleFiles(files,run,meth,pkey,std){
 if(!state.project.runFolders.includes(run))state.project.runFolders.push(run);
 for(const f of files){const name=f.name.replace(/\.(fsa|ab1|hid)$/i,'');
  state.project.samples.push({id:state.nextId++,file:f.name,name,type:ProjectOptions.get().add.type!=='auto'?ProjectOptions.get().add.type:sampleTypeFromName(f.name),typeAuto:ProjectOptions.get().add.type==='auto',
   runFolder:run,method:meth,panelKey:pkey,panel:pkey?pkey.split('\u0001')[1]:'',sizeStd:std,status:'unanalyzed',blob:f,origin:originOf(f),analysis:null,report:null})}
 Sec.record(files.map(f=>({map:'Sample',attribute:'created',instance:`${run}/${f.name}`,old:'',new:f.name,event:'Add Sample'})));
 sniffSamples(state.project.samples);
 state.project.dirty=true;render()}

// ---------- Edit > Define New Sample File Path ----------
// The sample keeps a reference to its .fsa (Sample Origin Path = folder it was added from; browsers expose only the
// folder-relative part). A missing file (SFNF) is re-associated by choosing the folder that now holds it: files are
// matched by name (case-insensitive) and, when the sample was analysed, by the SHA-256 of the file contents.
const originOf=f=>{const r=f.webkitRelativePath||'';return r.includes('/')?r.slice(0,r.lastIndexOf('/')):''};
async function sha256Hex(blob){const d=await crypto.subtle.digest('SHA-256',await blob.arrayBuffer());return [...new Uint8Array(d)].map(b=>b.toString(16).padStart(2,'0')).join('')}
async function associateFiles(targets,files){
 const byName=new Map();for(const f of files){if(!/\.(fsa|ab1|hid)$/i.test(f.name))continue;const k=f.name.toLowerCase();if(!byName.has(k))byName.set(k,[]);byName.get(k).push(f)}
 const res={ok:[],missing:[],differs:[]};
 for(const s of targets){const cands=byName.get(String(s.file).toLowerCase())||[];
  if(!cands.length){res.missing.push(s.file);continue}
  const want=s.analysis?.source_sha256||s.compact?.source_sha256;let pick=null;
  if(want){for(const f of cands){try{if((await sha256Hex(f))===want){pick=f;break}}catch(e){}}if(!pick){res.differs.push(s.file);continue}}
  else pick=cands[0];
  const was=s.origin||'';s.blob=pick;s.inst=null;s.origin=originOf(pick)||was;s.rev=(s.rev||0)+1;res.ok.push(s);
  Sec.record([{map:'Sample',attribute:'sample file path',instance:`${s.runFolder}/${s.file}`,old:was,new:s.origin||pick.name,event:'Define New Sample File Path'}])}
 if(res.ok.length){state.project.dirty=true;SampleViews?.invalidate?.();sniffSamples(res.ok)}
 return res}
function defineSamplePath(){
 const targets=state.project.samples.filter(s=>state.selected.has(s.id)&&s.method!==EXAMPLE_METHOD);
 if(!targets.length){message(T('dspSelect'));return}
 const input=el('input',{type:'file',webkitdirectory:'',directory:'',multiple:'',style:'display:none'});document.body.append(input);
 input.addEventListener('change',async()=>{const files=[...input.files];input.remove();if(!files.length)return;
  $('statusText').textContent=T('dspChecking');
  const r=await associateFiles(targets,files);render();
  const lines=[T('dspDone').replace('{0}',r.ok.length).replace('{1}',targets.length)];
  if(r.missing.length)lines.push(T('dspMissing')+'\n  '+r.missing.slice(0,20).join('\n  ')+(r.missing.length>20?'\n  …':''));
  if(r.differs.length)lines.push(T('dspDiffers')+'\n  '+r.differs.slice(0,20).join('\n  ')+(r.differs.length>20?'\n  …':''));
  $('statusText').textContent=lines[0];message(lines.join('\n\n'),T('dspTitle'))},{once:true});
 input.click()}

// ---------- analysis workflow: requirements (ARS) -> sizing -> ladders (ALAS) -> genotyping -> summary ----------
function requirementsSummary(failing){
 // Analysis Requirements Summary: resolves 'continue' | 'view' | 'cancel'.
 return new Promise(res=>{const counts=new Map();for(const s of failing)for(const k of arnmCodes(s)){if(!counts.has(k))counts.set(k,[]);counts.get(k).push(s.id)}
  const view=ids=>{state.filter={ids:new Set(ids),label:T('arsTitle')};showTab('samples')};
  const d=dialog(T('arsTitle'),b=>{b.classList.add('arsdlg');
   const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},el('th',{text:T('arsRequirement')}),el('th',{text:T('samples')}),el('th',{text:T('arsEffect')}))));const tb=el('tbody');
   for(const [k,ids] of counts){const a=el('a',{text:String(ids.length),href:'#'});a.addEventListener('click',e=>{e.preventDefault();d.close();view(ids);res('view')});
    tb.append(el('tr',{},el('td',{text:T(k)}),el('td',{class:'num'},a),el('td',{text:T(BLOCKING.has(k)?'arsNotAnalyzed':'arsAnalyzedOnly')})))}
   t.append(tb);b.append(el('p',{text:T('arsIntro').replace('{0}',failing.length)}),el('div',{class:'grid'},t))},
   [[T('arsView'),()=>{view(failing.map(s=>s.id));res('view');return true}],[T('arsContinue'),()=>{res('continue');return true},true],[T('cancel'),()=>{res('cancel');return true}]]);
  d.root.querySelector('.x').addEventListener('click',()=>res('cancel'))})}

// Samples Not Normalized (ID-X NormalizationWarningDialog): the method asks for normalization but the sample file
// carries no size-standard normalization factor; "Don't show again until the next session".
function normalizationWarning(samples){
 let hide=false;try{hide=sessionStorage.getItem('opentrace.hideNormWarning')==='1'}catch(e){}
 if(hide)return;
 const bad=samples.filter(s=>s.analysis&&Methods.get(s.method)?.peak_detector?.normalization&&s.analysis.normalization?.sample_file!=='Yes');
 if(!bad.length)return;
 const all=bad.length===samples.filter(s=>s.analysis&&Methods.get(s.method)?.peak_detector?.normalization).length;
 dialog(T('nwTitle'),b=>{b.classList.add('nwdlg');b.append(el('p',{text:T(all?'nwAll':'nwSome')}),el('p',{text:T('nwNotApplied').replace('{0}',bad.length)}),
   el('label',{class:'chk'},el('input',{type:'checkbox',id:'nwHide'}),document.createTextNode(' '+T('nwHide'))))},
  [[T('ok'),body=>{if(body.querySelector('#nwHide').checked)try{sessionStorage.setItem('opentrace.hideNormWarning','1')}catch(e){}return true},true]])}
const ANALYZE_CONCURRENCY=4;
async function analyze(scope){
 let targets=state.project.samples.filter(s=>scope==='all'||(scope==='selected'?state.selected.has(s.id):s.status!=='analyzed'));
 if(!targets.length||state.busy)return;if(!Sec.need('UPDATE_PROJECT'))return;
 const opts=ProjectOptions.get();
 const failing=targets.filter(s=>arnmCodes(s).some(k=>k!=='arnmLadder'));
 if(failing.length&&opts.ars==='stop'){const r=await requirementsSummary(failing);if(r!=='continue')return}
 targets=targets.filter(s=>!blocked(s));
 if(!targets.length)return;
 const edited=targets.filter(s=>activeEdits(s).length);
 if(edited.length){if(!(await Methods.ask(T('reanalyzeDropsEdits').replace('{0}',edited.length))))return;
  for(const s of edited)for(const e of s.edits)if(!e.undone){e.undone=true;e.undoneAt=new Date().toISOString();e.undoneBy='reanalysis'}}
 state.busy=true;state.stop=false;state.lastMessage=null;state.noLadder=null;state.binSetNote=null;state.genotypeErrors=[];render();
 const order=[...targets].sort((a,b)=>(a.type==='ladder'?0:1)-(b.type==='ladder'?0:1)); // ladders first
 let i=0,finished=false,next=0,failure=null;
 // Samples are independent: several requests in flight let the server's worker processes use more than one core.
 const analyzeOne=async s=>{
   progress(i,order.length,`${T('analyzing')}: ${s.file}`);
   if(s.blob){const p=new URLSearchParams({...(await analyzeSettings(s.method,s.sizeStd)),filename:s.file});
    if(s.sqOverride)p.set('sizing_override',JSON.stringify(s.sqOverride));
    if(s.inst?.is310&&s.matrix&&Matrices.get(s.matrix))p.set('matrix',JSON.stringify(Matrices.get(s.matrix).values));
    let r;try{r=await fetch('/api/analyze?'+p,{method:'POST',headers:{'X-Session-Token':TOKEN},body:s.blob})}
    catch(e){throw Error(`${s.file}: `+T('serverUnreachable').replace('{0}','/api/analyze')+' ('+e.message+')')}
    let j;try{j=await r.json()}catch(e){throw Error(`${s.file}: HTTP ${r.status}`)}
    // A file the core cannot read (damaged, or a .hid that does not decode) is reported and skipped; the other samples
    // of the run are still analysed (before, the first such file stopped the whole analysis).
    if(!r.ok&&r.status>=400&&r.status<500){s.analysis=null;s.compact=null;s.report=null;s.status='unanalyzed';s.readError=String(j.error||r.status);
     state.genotypeErrors.push(`${s.file}: ${s.readError}`);i++;progress(i,order.length);return}
    if(!r.ok)throw Error(`${s.file}: ${j.error}`);s.readError=null;s.analysis=slimAnalysis(j);s.compact=null;s.rev=(s.rev||0)+1}
   s.status='analyzed';i++;progress(i,order.length)};
 const lane=async()=>{while(!state.stop&&!failure){const s=order[next++];if(!s)return;try{await analyzeOne(s)}catch(e){failure=failure||e;return}}};
 try{
  await Promise.all(Array.from({length:Math.min(ANALYZE_CONCURRENCY,order.length)},lane));
  if(failure)throw failure;
  normalizationWarning(order);
  finished=await genotype();
  const any=state.project.samples.some(s=>s.panelKey);
  const skipped=state.project.samples.filter(s=>s.alasSkipped).length;
  state.lastMessage=skipped?T('alasSkippedMsg').replace('{0}',skipped):state.noLadder?T('noLadderInRun')+' '+state.noLadder:any?T('analysisDone')+(state.binSetNote?' — '+T('binSetSubstituted')+' '+state.binSetNote:''):T('noKit');
  if(state.genotypeErrors.length)message(T('genotypeErrors')+'\n'+state.genotypeErrors.slice(0,20).join('\n')+(state.genotypeErrors.length>20?'\n…':''));
  Sec.record([{map:'Project',attribute:'analyzed',instance:state.project.name||'',old:'',new:`${i}/${order.length}`,event:'Analyze Project'}]);
 }catch(err){message(err.message)}
 finally{state.busy=false;state.project.dirty=true;
  if(finished&&!state.stop){if(opts.lowQualityTop)state.lowQualityTop=true;if(finished!=='view'){state.tab=opts.after==='summary'?'summary':'samples';renderTabs()}}
  render()}}

// One genotyping group = run folder + panel. Ladders are judged first with a ladder as the probe sample.
function groups(){const out=[];
 for(const rf of state.project.runFolders){const inRun=state.project.samples.filter(s=>s.runFolder===rf&&s.analysis&&s.status==='analyzed'&&s.panelKey);
  for(const key of [...new Set(inRun.map(s=>s.panelKey))])out.push({rf,key,group:inRun.filter(s=>s.panelKey===key)})}
 return out}
const ladderInput=s=>({ladder_id:s.file,analysis:s.compact||compactAnalysis(s.analysis)});
const sqPass=s=>{const q=samplePqv(s).SQ;return q==='pass'||q==='override'};
async function judgeLadders(g){
 const lad=g.group.filter(s=>s.type==='ladder');
 for(const s of lad){s.ladderQuality=null;s.ladderUsed=false}
 g.ladders=lad.map(ladderInput);g.pass=false;
 const probe=lad.find(s=>s.analysis?.sizing?.status==='accepted');if(!probe)return g;
 const k=await kitFor(g.key,probe.method);
 const rep=await jsonPost('/api/str-run',{sample:ladderInput(probe).analysis,ladders:g.ladders,kit:k.kit,ladder_quality_definition:k.lq,missing_offset_policy:k.policy});
 const used=new Set(Object.values(rep.ladder?.markers||{}).flatMap(m=>m.status==='accepted'?m.source_ladders||[]:[]));
 for(const l of rep.quality?.run_ladders||[]){const s=lad.find(x=>x.file===l.ladder_id);if(s){s.ladderQuality=l.quality_code;s.ladderUsed=used.has(s.file)}}
 g.pass=['accepted','partial'].includes(rep.ladder?.status)&&lad.some(s=>s.ladderQuality===0&&sqPass(s));
 // ID-X bin offsets: passing allelic ladders with green SQ (Project Options > Bin Offsets: or green and/or yellow SQ) contribute.
 const bo=ProjectOptions.get().binOffsets;const green=lad.filter(s=>sqPass(s)||(bo==='greenYellow'&&samplePqv(s).SQ==='check'));let offRep=rep;g.offLadders=g.ladders;
 if(green.length&&green.length<lad.length){g.offLadders=green.map(ladderInput);
  const gp=green.find(s=>s.analysis?.sizing?.status==='accepted')||probe;
  try{offRep=await jsonPost('/api/str-run',{sample:ladderInput(gp).analysis,ladders:g.offLadders,kit:k.kit,ladder_quality_definition:k.lq,missing_offset_policy:k.policy});
   g.pass=g.pass&&['accepted','partial'].includes(offRep.ladder?.status)}catch(e){g.offLadders=g.ladders;offRep=rep}}
 recordBinOffsets(g,k,offRep.ladder);
 return g}
// Bin offsets per run folder and panel (View > Bin Offsets): Run, Bin Set, Panel, Marker, Bin, Offset.
function recordBinOffsets(g,k,ladder){
 const list=(state.project.binOffsets||[]).filter(e=>!(e.run===g.rf&&e.panelKey===g.key));
 const ok=ladder&&['accepted','partial'].includes(ladder.status);
 const markers=ok?(k.kit?.markers||[]).map(m=>{const r=ladder.markers?.[m.name];if(!r||r.status!=='accepted'||!r.bin_offsets)return null;
  return {marker:m.name,bins:Object.entries(r.bin_offsets).map(([b,v])=>[b,Number(v)])}}).filter(Boolean):[];
 if(markers.length)list.push({run:g.rf,panelKey:g.key,panel:k.kit?.name||g.group[0]?.panel||'',binSet:k.binSet||'',markers});
 state.project.binOffsets=list}
function binOffsetRows(rf){
 const out=[];for(const e of (state.project.binOffsets||[]).filter(x=>x.run===rf))for(const m of e.markers)for(const [b,v] of m.bins)
  out.push([e.run,e.binSet,e.panel,m.marker,b,String(Math.round(v*1e4)/1e4)]);return out}
function binOffsetsDialog(){
 const rf=state.filter?.run;if(!rf)return;
 const head=[T('boRun'),T('boBinSet'),T('boPanel'),T('boMarker'),T('boBin'),T('boOffset')];const rows=binOffsetRows(rf);
 dialog(T('boTitleRun').replace('{0}',rf),b=>{b.classList.add('bodlg');
  const t=el('table',{class:'grid'});t.append(el('thead',{},el('tr',{},el('th',{text:''}),...head.map(h=>el('th',{text:h})))));
  const tb=el('tbody');rows.forEach((r,i)=>tb.append(el('tr',{},el('td',{class:'rn',text:String(i+1)}),...r.map((c,j)=>el('td',{text:c,class:j===5?'num':''})))));
  t.append(tb);b.append(el('div',{class:'bowrap'},t));if(!rows.length)b.append(el('p',{class:'note',text:T('boEmpty')}))},
  [[T('boExport'),()=>{try{saveText(`${rf.replace(/[\\/:*?"<>|]+/g,'_')}_BinOffsets.txt`,[head,...rows].map(r=>r.join('\t')).join('\n')+'\n','txt')}catch(e){message(T('boExportError')+': '+e.message)}return false},false,],
   [T('close'),()=>true,true]])}
function ladderSummary(gs){
 // Allelic Ladder Analysis Summary: resolves 'passing' | 'all' | 'view'.
 return new Promise(res=>{
  const lowLadders=gs.flatMap(g=>g.group.filter(s=>s.type==='ladder'&&!(s.ladderQuality===0&&sqPass(s)))).map(s=>s.id);
  const d=dialog(T('alasTitle'),b=>{b.classList.add('alasdlg');b.append(el('p',{text:T('alasIntro')}),ladderTable(gs.map(g=>g.rf),{group:true}),el('p',{class:'note',text:T('alasNote')}))},
   [[T('alasPassing'),()=>{res('passing');return true},true],[T('alasAll'),()=>{res('all');return true}],
    [T('alasView'),()=>{state.filter={ids:new Set(lowLadders),label:T('alasTitle')};showTab('samples');res('view');return true}]]);
  d.root.querySelector('.x').addEventListener('click',()=>res('passing'))})}
async function genotype(){
 const gs=groups();if(!gs.length)return true;
 for(const s of state.project.samples){if(gs.some(g=>g.group.includes(s))){s.alasSkipped=false;s.referenceBins=false}}
 for(const g of gs){if(state.stop)return false;$('statusText').textContent=`${T('ladderEval')}: ${g.rf}`;await judgeLadders(g)}
 const failing=gs.filter(g=>!g.pass);let mode='passing';
 if(failing.length){const o=ProjectOptions.get().alas;mode=o==='stop'?await ladderSummary(gs):o;
  if(mode==='view'){for(const g of failing)for(const s of g.group)if(s.type!=='ladder')s.report=null;return 'view'}}
 for(const g of gs){
  if(!g.pass&&mode!=='all'){for(const s of g.group)if(s.type!=='ladder'){s.report=null;s.alasSkipped=true;s.rev=(s.rev||0)+1}
   if(!g.ladders.length)state.noLadder=g.rf;continue}
  const methods=[...new Set(g.group.filter(s=>s.type!=='ladder').map(s=>s.method))];
  const kits={};for(const mname of methods)kits[mname]=await kitFor(g.key,mname);
  const todo=g.group.filter(x=>x.type!=='ladder');let at=0;
  // The run's ladders are identical for every sample: send them once and let the server cache the ladder cohort.
  const register=async()=>{try{return (await jsonPost('/api/ladder-set',{ladders:g.offLadders||g.ladders})).ladder_set}catch(e){return null}};
  let ladderSet=await register();
  const one=async s=>{
   const k=kits[s.method];if(k.binSetSubstituted)state.binSetNote=k.binSet;
   $('statusText').textContent=`${T('genotyping')}: ${s.file}`;
   const policy=g.pass?k.policy:'reference_bins';
   let rep;
   // Marker quality (and the slimmed report) come back in the same round trip instead of echoing a multi-MB report to /api/marker-quality.
   let mqDef=null;
   if(k.mq){mqDef=k.mq;const accW=Number(Methods.get(s.method)?.sq_gq?.weights?.acc||0);
    // Samples: AMEL cross check weight of the method enters GQ (ID-X: "Effect on GQ (samples only)").
    if(s.type==='sample'&&k.mq.amel_marker&&accW>0)mqDef={...k.mq,schema_version:2,sample_type:'Sample',control_mode:'none',amel_cross_check:{weight:accW}}}
   const body=()=>({sample:s.compact||compactAnalysis(s.analysis),...(ladderSet?{ladder_set:ladderSet}:{ladders:g.offLadders||g.ladders}),kit:k.kit,ladder_quality_definition:k.lq,missing_offset_policy:policy,marker_quality_definition:mqDef,slim:true,pullup_excludes_calls:ProjectOptions.get().pullupExcludesCalls===true,homozygous_min_height:ProjectOptions.get().homoMinHeight,heterozygous_min_height:ProjectOptions.get().hetMinHeight});
   try{try{rep=await jsonPost('/api/str-run',body())}catch(e){if(ladderSet&&/ladder_set/.test(e.message)){ladderSet=await register();rep=await jsonPost('/api/str-run',body())}else throw e}}
   catch(e){s.report=null;s.genotypeError=e.message;state.genotypeErrors=(state.genotypeErrors||[]).concat([`${s.file}: ${e.message}`]);s.rev=(s.rev||0)+1;return}
   s.genotypeError=null;
   // A sample that failed sizing (or another sample check) has no allele calls: keep the report with its reasons, skip GQ/control scoring.
   if(rep.allele_calling&&(s.type==='positive'||s.type==='negative')){const def=controlDefinition(s);if(def)try{rep=await jsonPost('/api/controls',{report:rep,definition:def})}catch(e){rep.quality=rep.quality||{};rep.quality.controls_error=e.message}}
   s.report=slimReport(rep);s.referenceBins=!g.pass;s.rev=(s.rev||0)+1};
  // independent samples: several requests in flight keep the server's worker processes busy
  await Promise.all(Array.from({length:Math.min(ANALYZE_CONCURRENCY,todo.length)},async()=>{while(!state.stop){const s=todo[at++];if(!s)return;await one(s)}}));
  if(state.stop)return false}
 return true}

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
 const po=ProjectOptions.get();
 const mode=state.tab==='genotypes'?'genotypes':'samples';
 if(mode==='genotypes'&&!state.genoSel.length)state.genoSel=state.project.samples.filter(s=>state.selected.has(s.id)).flatMap(s=>genoRows(s).map(r=>s.id+'|'+r.marker));
 return {lang:I18N.lang,mode,genoSel:mode==='genotypes'?state.genoSel.slice():[],project:state.project.name||'',printComment:po.printComment?po.comment:'',kit:(samples.map(sampleKit).find(Boolean))||null,samples:samples.map(s=>{const eff=s.report?effective(s):null;const rows=genoRows(s);
  return {id:s.id,file:s.file,name:s.name,type:s.type,analysis:s.analysis,report:s.report?{...s.report,calls:eff.calls,artifacts:eff.artifacts}:null,spk:s.report?spikeArtifacts(s):[],kit:sampleKit(s),pqv:sampleFlags(s),omr:s.report?omrPeaks(s):[],
   editReasons:Object.fromEntries(activeEdits(s).map(e=>[e.callId||('u'+e.id),e.reason||''])),
   markers:rows.map(r=>({marker:r.marker,channel:r.channel,gq:r.gq,gqCat:r.flags.GQ,flags:r.flags,panel:s.panel||'',calls:r.calls,edited:r.flags.ME==='check',pli:r.flags.PLI==='check'})),editable:!!s.report}})}}
function openPanelManager(){if(!Sec.need('READ_PANEL'))return;const w=window.open('/idx/panels.html','opentrace-panels','width=1180,height=800');if(!w)message(T('plotWindowBlocked'));else w.focus()}
function displayPlots(){
 if(state.tab==='genotypes'&&!state.genoSel.length&&state.selected.size)plotPayload();
 if(state.tab==='genotypes'?!state.genoSel.length:!state.selected.size)return message(T(state.tab==='genotypes'?'selectMarkersFirst':'selectSamplesFirst'));
 const w=window.open('/idx/plot.html','opentrace-plot','width=1100,height=760');
 if(!w)message(T('plotWindowBlocked'));else{state.plotWin=w;w.focus()}
 ensureKitsQuiet(state.project.samples.filter(s=>state.selected.has(s.id)))}

// ---------- export format (ID-X "Format of Exported, Copied, and Report Results") ----------
const STATUS_TEXT={analyzed:'Analyzed',unanalyzed:'Not Analyzed',changed:'Analysis setting changed'};
const BOOL_COLS=new Set(['SE','SQO','ADO','ME','PLI']);
const TYPE_TEXT={sample:'Sample',ladder:'Allelic Ladder',positive:'Positive Control',negative:'Negative Control'};
const pqvNumber=v=>v==='override'?0.5:PQV_NUM[v]??-2;
function exportSampleValue(id,s,flags){
 if(id==='status')return STATUS_TEXT[s.status]||'';
 if(id==='sampleType')return TYPE_TEXT[s.type]||s.type;
 const v=sampleCell(id,s,flags);
 if(BOOL_COLS.has(id))return v.value==='na'?'':v.value==='check'?'TRUE':'FALSE';
 if(id==='SQ'){const f=flags.SQ;if(f==='na')return -2;const q=sqScore(s);return q==null?(f==='override'?1:pqvNumber(f)):Number(q.toFixed(3))}
 if(v.html!=null&&PQV_NUM[v.value]!==undefined||v.value==='override')return pqvNumber(v.value);
 return v.value??''}
function exportGenoValue(id,r,i){
 if(BOOL_COLS.has(id)){const f=r.flags?.[id]??'na';return f==='na'?'':f==='check'?'TRUE':'FALSE'}
 if(id==='GQ'){const f=r.flags?.GQ??'na';if(f==='override')return 0.5;return r.gq!=null?Number(r.gq.toFixed(3)):-2}
 const v=genoCell(id,r,i);
 if(v.html!=null&&PQV_NUM[v.value]!==undefined)return pqvNumber(v.value);
 return v.value??''}
const blankRow=s=>({sample:s,marker:'',channel:'',calls:[],gq:null,flags:{},peaks:[],reason:'',mec:''});
function tableRows(tab,onlySelected){const rows=[];
 if(tab==='samples'){const cols=sampleColumns();rows.push(cols.map(c=>c.head));
  for(const s of sortedSamples())if(!onlySelected||state.selected.has(s.id)){const f=sampleFlags(s);rows.push(cols.map(c=>exportSampleValue(c.base,s,f)))}}
 else{const cols=genoColumns();rows.push(cols.map(c=>c.head));
  for(const r of sortedGenoRows())if(!onlySelected||state.selected.has(r.sample.id))rows.push(cols.map(c=>exportGenoValue(c.base,r,c.index||0)))}
 return rows}
const csvCell=x=>{const s=x==null?'':String(x);return /[",\r\n]/.test(s)?'"'+s.replace(/"/g,'""')+'"':s};
function serialize(rows,type){return rows.map(r=>r.map(x=>type==='csv'?csvCell(x):(x==null?'':String(x).replace(/[\t\r\n]+/g,' '))).join(type==='csv'?',':'\t')).join('\r\n')+'\r\n'}
function saveText(name,text,type){const url=URL.createObjectURL(new Blob([text],{type:type==='csv'?'text/csv':'text/plain'}));
 const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
const fileBase=()=>(state.project.name||'project').replace(/[\\/:*?"<>|]+/g,'_');
function exportTable(){
 const tab=state.tab==='genotypes'?'genotypes':'samples';let type='txt';
 dialog(T('exportTable').replace(/\.+$/,''),b=>{b.append(el('div',{class:'field'},el('label',{text:T('exportType')}),typeSelect('etType',v=>type=v)),
  el('div',{class:'field'},el('label',{text:T('fileName')}),el('input',{type:'text',id:'etName',value:`${fileBase()}_${tab}`})),el('p',{class:'note',text:T('exportPqvNote')}))},
  [[T('exportBtn'),b=>{const name=b.querySelector('#etName').value.trim()||fileBase();saveText(`${name}.${type}`,serialize(tableRows(tab,false),type),type)},true],[T('cancel'),()=>true]])}
// Export Table with Stutter (Genotypes): the main alleles plus the peaks removed as stutter, labelled with their bins;
// not for allelic ladders, stutter peaks labelled as artifacts are left out, PQVs stay those of the main alleles.
function stutterRows(ids){const cols=genoColumns();const ts=TableSettings.active().genotypes;
 const rows=sortedGenoRows().filter(r=>!ids||ids.includes(r.sample.id)).map(r=>{if(r.sample.type==='ladder')return r;
  const am=(r.sample.report?.allele_calling?.markers||[]).find(m=>m.name===r.marker);const arts=effective(r.sample).artifacts;
  const st=(am?.stutter_calls||[]).filter(c=>c.label&&!arts.some(a=>a.channel===c.channel&&Math.abs(a.size_bp-c.size_bp)<0.05)).map(c=>({...c,marker:r.marker,stutter:true}));
  return {...r,calls:[...r.calls,...st].sort((a,b)=>a.size_bp-b.size_bp)}});
 const n=Math.max(ts.numAlleles,...rows.map(r=>r.calls.length));
 const out=[];const by=Object.fromEntries(TableSettings.CATALOG.genotypes.map(c=>[c.id,c]));const exp=[];
 for(const id of ts.visible){const c=by[id];if(c.kind==='allele')for(let i=0;i<n;i++)exp.push({...c,base:id,index:i,head:`${c.head} ${i+1}`});else exp.push({...c,base:id})}
 out.push(exp.map(c=>c.head));for(const r of rows)out.push(exp.map(c=>exportGenoValue(c.base,r,c.index||0)));return out}
function exportWithStutter(ids){let type='txt';
 dialog(T('exportWithStutter'),b=>{b.append(el('div',{class:'field'},el('label',{text:T('exportType')}),typeSelect('esType',v=>type=v)),
  el('div',{class:'field'},el('label',{text:T('fileName')}),el('input',{type:'text',id:'esName',value:`${fileBase()}_genotypes_stutter`})),el('p',{class:'note',text:T('stutterExportNote')}))},
  [[T('exportBtn'),b=>{const name=b.querySelector('#esName').value.trim()||fileBase();saveText(`${name}.${type}`,serialize(stutterRows(ids),type),type)},true],[T('cancel'),()=>true]])}
function typeSelect(id,onChange){const s=el('select',{id});s.append(new Option(T('typeTxt'),'txt'),new Option(T('typeCsv'),'csv'));s.addEventListener('change',()=>onChange(s.value));return s}

// ---------- combined export ----------
const SAMPLE_LEVEL=new Set(['sampleFile','sampleName','sampleID','runName','panel','profileId','UD1','UD2','UD3','UD4','UD5','UD6','UD7','UD8','UD9']);
const markerNames=list=>{const kit=list.map(sampleKit).find(Boolean);const names=kit?kit.markers.map(m=>m.name):[];
 for(const s of list)for(const r of genoRows(s))if(!names.includes(r.marker))names.push(r.marker);return names};
function alleleTable(list,{separate=false,sCols,gCols,numAlleles}){
 const kinds=gCols.filter(c=>c.kind==='allele');const bases=[...new Set(kinds.map(c=>c.base))];if(!bases.length)bases.push('allele');
 const head=base=>TableSettings.CATALOG.genotypes.find(c=>c.id===base).head;
 const markers=markerNames(list);const header=sCols.map(c=>c.head);
 markers.forEach((m,k)=>{if(separate){header.push(`Marker ${k+1}`);for(const b of bases)for(let i=0;i<numAlleles;i++)header.push(`${head(b)} ${i+1}`)}
  else for(const b of bases)header.push(b==='allele'?m:`${m} ${head(b)}`)});
 const rows=[header];
 for(const s of list){const f=sampleFlags(s);const by=Object.fromEntries(genoRows(s).map(r=>[r.marker,r]));const row=sCols.map(c=>exportSampleValue(c.base,s,f));
  for(const m of markers){const r=by[m]||{...blankRow(s),marker:m};
   if(separate){row.push(m);for(const b of bases)for(let i=0;i<numAlleles;i++)row.push(exportGenoValue(b,r,i))}
   else for(const b of bases){const n=Math.max(r.calls.length,r.calls.length===1&&ProjectOptions.get().dupHomo&&autosomal(m)?2:0);
    row.push(Array.from({length:n},(_,i)=>exportGenoValue(b,r,i)).filter(x=>x!=='').join(', '))}}
  rows.push(row)}
 return rows}
function combinedRows(mode,{separate,includeAll}){
 const sCols=sampleColumns();const gAll=genoColumns();const gCols=gAll.filter(c=>!SAMPLE_LEVEL.has(c.base));
 const list=sortedSamples();const numAlleles=TableSettings.active().genotypes.numAlleles;
 if(mode==='allele'){const files=[];const panels=[...new Set(list.map(s=>s.panel||''))];
  for(const p of panels){const sub=list.filter(s=>(s.panel||'')===p);files.push({panel:p,rows:alleleTable(sub,{separate,sCols,gCols:gAll,numAlleles})})}
  return files}
 if(mode==='sample'){const markers=markerNames(list);const header=sCols.map(c=>c.head);for(const m of markers)for(const c of gCols)if(c.base!=='marker')header.push(`${m} ${c.head}`);
  const rows=[header];
  for(const s of list){const f=sampleFlags(s);const by=Object.fromEntries(genoRows(s).map(r=>[r.marker,r]));const row=sCols.map(c=>exportSampleValue(c.base,s,f));
   for(const m of markers){const r=by[m]||{...blankRow(s),marker:m};for(const c of gCols)if(c.base!=='marker')row.push(by[m]?exportGenoValue(c.base,r,c.index||0):'')}
   rows.push(row)}
  return [{panel:'',rows}]}
 const header=[...sCols.map(c=>c.head),...gCols.map(c=>c.head)];const rows=[header];
 for(const s of list){const f=sampleFlags(s);const base=sCols.map(c=>exportSampleValue(c.base,s,f));const gr=genoRows(s);
  if(gr.length)for(const r of gr)rows.push([...base,...gCols.map(c=>exportGenoValue(c.base,r,c.index||0))]);
  else if(includeAll&&sampleKit(s))for(const m of sampleKit(s).markers)rows.push([...base,...gCols.map(c=>c.base==='marker'?m.name:'')]);
  else rows.push([...base,...gCols.map(()=>'')])}
 return [{panel:'',rows}]}
function exportCombined(){
 if(!state.project.samples.length)return message(T('selectSamplesFirst'));
 let type='txt',mode='allele',separate=false,includeAll=false;
 dialog(T('exportCombined').replace(/\.+$/,''),b=>{b.classList.add('ecdlg');
  const radio=(name,value,label,cur,set)=>{const r=el('input',{type:'radio',name,value,id:`ec_${name}_${value}`});r.checked=cur===value;r.addEventListener('change',()=>{if(r.checked)set(value)});return el('label',{class:'chk'},r,document.createTextNode(' '+label))};
  const inc=el('input',{type:'checkbox',id:'ecIncludeAll'});inc.addEventListener('change',()=>{includeAll=inc.checked});
  b.append(el('div',{class:'field'},el('label',{text:T('exportType')}),typeSelect('ecType',v=>type=v)),
   el('div',{class:'field'},el('label',{text:T('fileName')}),el('input',{type:'text',id:'ecName',value:`${fileBase()}_combined`})),
   el('fieldset',{},el('legend',{text:T('ecMerge')}),
    radio('mode','allele',T('ecAlleleTable'),mode,v=>mode=v),
    el('div',{class:'ind'},radio('sep','group',T('ecGroup'),'group',()=>separate=false),radio('sep','separate',T('ecSeparate'),'group',()=>separate=true)),
    radio('mode','sample',T('ecPerSample'),mode,v=>mode=v),radio('mode','marker',T('ecPerMarker'),mode,v=>mode=v),
    el('div',{class:'ind'},el('label',{class:'chk'},inc,document.createTextNode(' '+T('ecIncludeAll'))))),
   el('p',{class:'note',text:T('exportPqvNote')}))},
  [[T('exportBtn'),b=>{const name=b.querySelector('#ecName').value.trim()||fileBase();const files=combinedRows(mode,{separate,includeAll});
    const names=[];for(const f of files){const n=`${name}${f.panel?'_'+f.panel.replace(/[\\/:*?"<>|]+/g,'_'):''}.${type}`;saveText(n,serialize(f.rows,type),type);names.push(n)}
    if(mode==='allele')message(T('ecFilesCreated')+'\n'+names.join('\n'))},true],[T('cancel'),()=>true]])}

// ---------- Export Project Panel (ID-X ProjectPanelExporter) ----------
// Copies the project's panel under new, unique Chemistry Kit / Panel names and writes <Kit>_Panel.txt and <Kit>_Bin.txt (bin set <Kit>_Bin_Set).
const validName=n=>/^[A-Za-z0-9][A-Za-z0-9_.\- ]*$/.test(n)&&n.length<=64;
function projectPanelFiles(panelKey,methodName,kitName,panelName){
 const db=KitStore.load();const [srcKit,srcPanel]=panelKey.split('\u0001');const kit=db.kits.find(k=>k.name===srcKit);const pn=kit?.panels.find(p=>p.name===srcPanel);
 if(!kit||!pn)throw Error(T('panelNotReady'));
 const method=methodName&&Methods.get(methodName);const usable=kit.binSets.filter(x=>x.panels[srcPanel]);
 const set=usable.find(x=>x.name===method?.allele?.bin_set)||usable[0];
 const out={name:kitName,type:kit.type||'MICROSATELLITE',comment:'',panelsVersion:kit.panelsVersion,panels:[{name:panelName,comment:'',markers:KitStore.clone(pn.markers)}],binSets:[],stutter:null};
 const binSet={name:kitName+'_Bin_Set',version:set?.version,panels:{[panelName]:KitStore.clone(set?.panels[srcPanel]||{})}};
 return {panel:KitStore.panelsText(out),bin:KitStore.binsText(out,binSet)}}
async function writeToFolder(files){ // [{name,text}] -> true when written, null when cancelled
 if(!window.showDirectoryPicker)return false;
 let dir;try{dir=await window.showDirectoryPicker({mode:'readwrite'})}catch(e){return null}
 let exists=false;for(const f of files){try{await dir.getFileHandle(f.name);exists=true}catch(e){}}
 if(exists&&!(await Methods.ask(T('epOverwrite'))))return null;
 for(const f of files){const h=await dir.getFileHandle(f.name,{create:true});const w=await h.createWritable();await w.write(f.text);await w.close()}
 return true}
function exportProjectPanel(){
 const used=[...new Map(state.project.samples.filter(x=>x.panelKey&&x.panelKey!==EXAMPLE_KEY).map(x=>[x.panelKey,x])).values()];
 if(!used.length)return message(T('epNoPanel'));
 const sel=state.project.samples.find(x=>state.selected.has(x.id)&&x.panelKey&&x.panelKey!==EXAMPLE_KEY)||used[0];
 let src=sel;const kits=KitStore.load().kits.map(k=>k.name);const panels=new Set(KitStore.load().kits.flatMap(k=>k.panels.map(x=>x.name)));
 dialog(T('epTitle'),b=>{b.classList.add('epdlg');
  const pick=el('select',{id:'epSource'});for(const u of used)pick.append(new Option(u.panelKey.split('\u0001').join(' / '),u.panelKey));pick.value=src.panelKey;pick.addEventListener('change',()=>{src=used.find(u=>u.panelKey===pick.value)});
  b.append(el('p',{text:T('epSpecify')}),...(used.length>1?[el('div',{class:'field'},el('label',{text:T('epSource')}),pick)]:[]),
   el('div',{class:'field'},el('label',{text:T('epKitName')}),el('input',{type:'text',id:'epKit',maxlength:'64'})),
   el('div',{class:'field'},el('label',{text:T('epPanelName')}),el('input',{type:'text',id:'epPanel',maxlength:'64'})))},
  [[T('ok'),async b=>{const kn=b.querySelector('#epKit').value.trim(),pn=b.querySelector('#epPanel').value.trim();
    if(!validName(kn)){message(T('epKitInvalid'));return false}
    if(!validName(pn)){message(T('epPanelInvalid'));return false}
    if(!kn||kits.includes(kn)){message(T('epKitUnique'));return false}
    if(pn.toLowerCase()==='none'||panels.has(pn)){message(T('epPanelUnique'));return false}
    try{const f=projectPanelFiles(src.panelKey,src.method,kn,pn);const files=[{name:`${kn}_Panel.txt`,text:f.panel},{name:`${kn}_Bin.txt`,text:f.bin}];
     const r=await writeToFolder(files);if(r===null)return true;
     if(r===false){for(const x of files)saveText(x.name,x.text,'txt')}
     message(T('epDone')+'\n'+files.map(x=>x.name).join('\n'));return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]])}

// ---------- copy ----------
function copyRows(){const tab=state.tab==='genotypes'?'genotypes':'samples';const rows=tableRows(tab,true);
 const text=rows.map(r=>r.map(x=>x==null?'':String(x)).join('\t')).join('\n');
 (navigator.clipboard?navigator.clipboard.writeText(text):Promise.reject()).then(()=>{state.lastMessage=T('copied')+': '+(rows.length-1);render()},()=>message(text))}

// ---------- reports (Report Manager window) ----------
function reportSamples(ids){const list=sortedSamples();if(ids&&ids.length){const set=new Set(ids);return state.project.samples.filter(s=>set.has(s.file))}
 const sel=list.filter(s=>state.selected.has(s.id));return sel.length?sel:list}
function reportData(setting,files){
 const list=reportSamples(files);const cols=setting.columns.filter(c=>c.show);
 const perGeno=cols.some(c=>c.cat==='Genotype');const rows=[];
 const value=(c,s,f,r)=>c.cat==='Custom'?'':c.cat==='Sample'?exportSampleValue(c.id,s,f):exportGenoValue(c.id,r,c.index||0);
 for(const s of list){const f=sampleFlags(s);const gr=perGeno?genoRows(s):[];
  if(gr.length)for(const r of gr)rows.push({file:s.file,name:s.name,panel:s.panel||'',marker:r.marker,values:cols.map(c=>value(c,s,f,r))});
  else rows.push({file:s.file,name:s.name,panel:s.panel||'',marker:'',values:cols.map(c=>value(c,s,f,blankRow(s)))})}
 const byMarker={markers:markerNames(list),samples:list.map(s=>({file:s.file,name:s.name,alleles:Object.fromEntries(genoRows(s).map(r=>[r.marker,r.calls.map(c=>c.label)]))}))};
 return {project:state.project.name||'',heads:cols.map(c=>ReportSettings.colHead(c)),cols,rows,byMarker,files:list.map(s=>s.file)}}
function openReportManager(){const w=window.open('/idx/report.html','opentrace-report','width=1100,height=720');if(!w)message(T('plotWindowBlocked'));else{state.reportWin=w;w.focus()}}
const reportApi={
 data:(setting,files)=>reportData(setting,files),
 list:()=>(state.project.reports||[]).map(r=>({name:r.name,setting:r.setting.name,saved:r.saved,samples:r.files.length})),
 get:name=>(state.project.reports||[]).find(r=>r.name===name)||null,
 save:rep=>{state.project.reports=(state.project.reports||[]).filter(r=>r.name!==rep.name);state.project.reports.push({...rep,saved:new Date().toISOString()});state.project.dirty=true;render();return true},
 remove:name=>{state.project.reports=(state.project.reports||[]).filter(r=>r.name!==name);state.project.dirty=true;render()},
 alleleTable:(setting,files,separate)=>{const list=reportSamples(files);const cat=TableSettings.CATALOG;
  const sCols=setting.columns.filter(c=>c.show&&c.cat==='Sample').map(c=>({...cat.samples.find(x=>x.id===c.id),base:c.id}));
  const gCols=[...new Set(setting.columns.filter(c=>c.show&&c.cat==='Genotype'&&ReportSettings.isAllele('Genotype',c.id)).map(c=>c.id))].map(id=>({...cat.genotypes.find(x=>x.id===id),base:id}));
  const out=[];for(const p of [...new Set(list.map(s=>s.panel||''))]){const sub=list.filter(s=>(s.panel||'')===p);out.push({panel:p,rows:alleleTable(sub,{separate,sCols,gCols,numAlleles:setting.numAlleles})})}
  return out},
 plot:files=>{const set=new Set(files);state.selected=new Set(state.project.samples.filter(s=>set.has(s.file)).map(s=>s.id));render();displayPlots()},
 hasProject:()=>!!state.project.samples.length};
window.OT.report=reportApi;window.OT.binOffsetRows=binOffsetRows;window.OT.associateFiles=associateFiles;window.OT.sniffSamples=sniffSamples;window.OT.defineSamplePath=defineSamplePath;window.OT.binOffsets=binOffsetsDialog;

// ---------- printing: page setup and print preview ----------
const PAGE_KEY='opentrace.pageSetup.v1';
function pageSetup(){let v={};try{v=JSON.parse(localStorage.getItem(PAGE_KEY)||'{}')||{}}catch(e){}
 const fz=Math.round(Number(v.fontSize));
 return {orientation:v.orientation==='portrait'?'portrait':'landscape',fit:v.fit!==false,header:v.header!==false,paper:['A4','Letter','Legal'].includes(v.paper)?v.paper:'A4',
  // ID-X Page Setup (tables): Use Screen Font / Font size, Print All Data, Start New Page, Print column first / row first
  screenFont:v.screenFont!==false,fontSize:fz>=6&&fz<=16?fz:10,printAll:v.printAll!==false,newPage:v.newPage===true,order:v.order==='row'?'row':'column'}}
function pageSetupDialog(){const o=pageSetup();
 dialog(T('pageSetup').replace(/\.+$/,''),b=>{b.classList.add('psdlg');
  const paper=el('select',{id:'psPaper'});for(const p of ['A4','Letter','Legal'])paper.append(new Option(p,p));paper.value=o.paper;paper.addEventListener('change',()=>o.paper=paper.value);
  const or=el('select',{id:'psOrient'});or.append(new Option(T('psLandscape'),'landscape'),new Option(T('psPortrait'),'portrait'));or.value=o.orientation;or.addEventListener('change',()=>o.orientation=or.value);
  const fit=el('input',{type:'checkbox',id:'psFit'});fit.checked=o.fit;fit.addEventListener('change',()=>o.fit=fit.checked);
  const hd=el('input',{type:'checkbox',id:'psHeader'});hd.checked=o.header;hd.addEventListener('change',()=>o.header=hd.checked);
  b.append(el('div',{class:'field'},el('label',{text:T('psPaper')}),paper),el('div',{class:'field'},el('label',{text:T('psOrientation')}),or),
   el('label',{class:'chk'},fit,document.createTextNode(' '+T('psFit'))),el('label',{class:'chk'},hd,document.createTextNode(' '+T('psHeaderText'))),
   (()=>{const chk=(id,key,label)=>{const c=el('input',{type:'checkbox',id});c.checked=o[key];c.addEventListener('change',()=>{o[key]=c.checked});return el('label',{class:'chk'},c,document.createTextNode(' '+T(label)))};
    const sf=el('input',{type:'checkbox',id:'psScreenFont'});sf.checked=o.screenFont;const fs=el('select',{id:'psFontSize'});for(let n=6;n<=16;n++)fs.append(new Option(String(n),String(n)));fs.value=String(o.fontSize);fs.disabled=o.screenFont;
    sf.addEventListener('change',()=>{o.screenFont=sf.checked;fs.disabled=sf.checked});fs.addEventListener('change',()=>{o.fontSize=Number(fs.value)});
    const radio=(v,label)=>{const r=el('input',{type:'radio',name:'psOrder',value:v,id:'psOrder_'+v});r.checked=o.order===v;r.addEventListener('change',()=>{if(r.checked)o.order=v});return el('label',{class:'chk'},r,document.createTextNode(' '+T(label)))};
    return el('fieldset',{},el('legend',{text:T('psTables')}),el('label',{class:'chk'},sf,document.createTextNode(' '+T('psScreenFont'))),el('div',{class:'field'},el('label',{text:T('psTblFontSize')}),fs),
     chk('psPrintAll','printAll','psPrintAll'),chk('psNewPage','newPage','psNewPage'),radio('column','psColumnFirst'),radio('row','psRowFirst'))})())},
  [[T('ok'),()=>{try{localStorage.setItem(PAGE_KEY,JSON.stringify(o))}catch(e){}},true],[T('cancel'),()=>true]])}
function printPayload(){
 const tab=state.tab;let title=T(tab==='summary'?'analysisSummary':tab),table=null,html=null;
 if(tab==='summary')html=$('summary').innerHTML;else{const pg=pageSetup();table=tableRows(tab==='genotypes'?'genotypes':'samples',!pg.printAll&&state.selected.size>0)}
 return {title,project:state.project.name||T('untitled'),date:new Date().toLocaleString(),page:pageSetup(),table,html,lang:I18N.lang}}
function printPreview(){const w=window.open('/idx/print.html','opentrace-print','width=1000,height=760');if(!w)message(T('plotWindowBlocked'));else w.focus()}
function printNow(){const w=window.open('/idx/print.html#print','opentrace-print','width=1000,height=760');if(!w)message(T('plotWindowBlocked'))}

// ---------- find and sort ----------
function findDialog(){
 const table=state.tab==='genotypes'?'genotypes':'samples';const cols=table==='samples'?sampleColumns():genoColumns();
 dialog(T('findTitle'),b=>{const col=el('select',{id:'fdCol'});col.append(new Option(T('allColumns'),''));for(const c of cols)col.append(new Option(c.head,cols.indexOf(c)));
  b.append(el('div',{class:'field'},el('label',{text:T('findWhat')}),el('input',{type:'text',id:'fdText'})),el('div',{class:'field'},el('label',{text:T('inColumn')}),col),
   el('label',{class:'chk'},el('input',{type:'checkbox',id:'fdCase'}),document.createTextNode(' '+T('matchCase'))))},
  [[T('selectMatches'),b=>{const q=b.querySelector('#fdText').value;if(!q)return false;const ci=b.querySelector('#fdCol').value;const mc=b.querySelector('#fdCase').checked;
    const norm=x=>mc?String(x):String(x).toLowerCase();const needle=norm(q);const use=ci===''?cols:[cols[+ci]];const hit=new Set();
    if(table==='samples')for(const s of visibleSamples()){const f=sampleFlags(s);if(use.some(c=>norm(sampleCell(c.base,s,f).value).includes(needle)))hit.add(s.id)}
    else for(const r of visibleSamples().flatMap(genoRows))if(use.some(c=>norm(genoCell(c.base,r,c.index||0).value).includes(needle)))hit.add(r.sample.id);
    state.selected=hit;state.lastMessage=`${T('found')}: ${hit.size}`;render();return true},true],[T('cancel'),()=>true]])}
function sortDialog(){
 const table=state.tab==='genotypes'?'genotypes':'samples';const cat=TableSettings.CATALOG[table];const cur=currentSort(table);
 dialog(T('sortTitle'),b=>{for(let i=0;i<3;i++){const sel=el('select',{'data-sort':String(i)});sel.append(new Option(T('sortNone'),''));for(const c of cat)sel.append(new Option(c.head,c.id));
   sel.value=cur[i]?.col||'';const dir=el('select');dir.append(new Option(T('ascending'),'0'),new Option(T('descending'),'1'));dir.value=cur[i]?.desc?'1':'0';
   b.append(el('div',{class:'field'},el('label',{text:T(i?'thenBy':'sortFirst')}),el('span',{class:'inl'},sel,dir)))}},
  [[T('ok'),b=>{const keys=[...b.querySelectorAll('[data-sort]')].map(x=>({col:x.value,desc:x.nextSibling.value==='1'})).filter(x=>x.col);
    state.sort={...(state.sort||{}),[table]:keys};state.lowQualityTop=false;render()},true],[T('cancel'),()=>true]])}
function tableSettingCombo(){const ts=el('select',{class:'combo',id:'tsCombo',title:T('tableSetting')});
 for(const r of TableSettings.list())ts.append(new Option(r.setting.name===TableSettings.DEFAULT.name?T('dataInterpretation'):r.setting.name,r.setting.name));
 ts.value=TableSettings.active().name;ts.addEventListener('change',()=>{state.sort=null;TableSettings.setActive(ts.value)});return ts}
function editTableSetting(){const s=TableSettings.active();
 TableSettings.openEditor(s,{onSave:(ns,prev)=>{TableSettings.put(ns,prev);TableSettings.setActive(ns.name)}})}

async function deleteFromProject(){
 if(!state.selected.size)return;if(!Sec.need('UPDATE_PROJECT'))return;
 const gone=state.project.samples.filter(s=>state.selected.has(s.id));
 const ch=await Sec.change({items:gone.map(s=>({map:'Sample',attribute:'deleted',instance:`${s.runFolder}/${s.file}`,old:s.file,new:'',label:s.file,event:'Delete Sample'}))});if(!ch)return;
 state.project.samples=state.project.samples.filter(s=>!state.selected.has(s.id));
 ch.commit();
 state.project.runFolders=state.project.runFolders.filter(rf=>state.project.samples.some(s=>s.runFolder===rf));
 state.selected.clear();state.project.dirty=true;render()}
function yScaleToDialog(){const r=SampleViews.yRange();let lo,hi;
 dialog(T('scaleTo').replace(/\.+$/,''),b=>{lo=el('input',{type:'number',id:'ysMin',value:String(Math.round(r.ymin))});hi=el('input',{type:'number',id:'ysMax',value:String(Math.round(r.ymax))});
  b.append(el('div',{class:'field'},el('label',{text:T('siMin')}),lo),el('div',{class:'field'},el('label',{text:T('siMax')}),hi))},
  [[T('ok'),()=>{const a=Number(lo.value),z=Number(hi.value);if(!(z>a)){message(T('rangeInvalid'));return false}SampleViews.yScaleTo(a,z);buildChrome()},true],[T('cancel'),()=>true]])}
function selectAll(){state.selected=new Set(visibleSamples().map(s=>s.id));render()}
function setLang(l){I18N.set(l);buildChrome();render()}

// ---------- chrome ----------
function buildChrome(){
 document.documentElement.lang=I18N.lang;
 const lang=I18N.lang;
 buildMenubar($('menubar'),[
  // Menu layout and order follow ProjectMgr of ID-X 1.6 (menu construction at the end of ProjectMgr.java). Items marked "extra" exist only in OpenTrace.
  {label:T('file'),items:[{label:T('newProject'),sc:'Ctrl+N',run:newProject},{label:T('openProject'),sc:'Ctrl+O',run:openProject},
   {label:T('saveProject'),sc:'Ctrl+S',run:saveProject},{label:T('saveProjectAs'),run:saveProjectAs},{label:T('closeProject'),run:closeProject}/*extra*/,'-',
   {label:T('exportTable'),sc:'Ctrl+E',run:exportTable},{label:T('exportCodis'),run:()=>Profiles.exportCodis()},{label:T('exportCombined'),run:exportCombined},
   {label:T('exportWithStutter')+'...',disabled:state.tab!=='genotypes',run:()=>exportWithStutter(null)}/*extra*/,{label:T('exportProjectPanel'),run:exportProjectPanel}/*extra: created but not placed in a menu in ID-X*/,'-',
   {label:T('pageSetup'),run:pageSetupDialog},{label:T('print'),sc:'Ctrl+P',run:printNow},{label:T('printPreview'),run:printPreview},'-',
   {label:T('projectOptions'),run:projectOptions},'-',
   {label:T('scChangePassword')+'...',run:()=>Sec.changePassword(),disabled:!Sec.user?.has_password}/*extra*/,{label:T('logOut'),run:()=>Sec.logout(),disabled:!Sec.state.require_login},{label:T('exit'),run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('undo'),sc:'Ctrl+Z',disabled:!histU.length,run:undoProject}/*extra*/,{label:T('redo'),sc:'Ctrl+Shift+Z',disabled:!histR.length,run:redoProject}/*extra*/,'-',
   {label:T('copy'),sc:'Ctrl+C',run:copyRows},{label:T('paste'),sc:'Ctrl+V',run:pasteCells},{label:T('clear'),sc:'Delete',run:clearCells}/*extra*/,'-',
   {label:T('addSamples'),sc:'Ctrl+K',run:addSamples},{label:T('associateSamples'),disabled:true},'-',
   {label:T('deleteFromProject'),run:deleteFromProject},{label:T('selectAll'),sc:'Ctrl+A',run:selectAll},{label:T('defineSamplePath'),run:defineSamplePath}/*extra*/,'-',
   {label:T('fillDown'),sc:'Ctrl+D',run:fillDown},{label:T('fitToContent'),sc:'Ctrl+I',run:fitToContent},'-',
   {label:T('find'),sc:'Ctrl+F',run:findDialog},{label:T('sort'),sc:'Ctrl+G',run:sortDialog}]},
  {label:T('analysis'),items:[{label:T('analyze'),sc:'Ctrl+R',run:()=>analyze('pending')},{label:T('analyzeSelected'),run:()=>analyze('selected')},{label:T('analyzeAll'),sc:'Ctrl+Shift+R',run:()=>analyze('all')},
   {label:T('analyzeMarkers'),disabled:true}]},
  {label:T('view'),items:[{label:T('samples'),sc:'Ctrl+Shift+1',checked:state.tab==='samples',run:()=>{buildAfter(()=>showTab('samples'))}},{label:T('analysisSummary'),sc:'Ctrl+Shift+2',checked:state.tab==='summary',run:()=>buildAfter(()=>showTab('summary'))},
   {label:T('genotypes'),sc:'Ctrl+Shift+3',checked:state.tab==='genotypes',run:()=>buildAfter(()=>showTab('genotypes'))},'-',
   {label:T('displayPlots'),sc:'Ctrl+L',run:displayPlots},{label:T('labelEditViewer'),run:labelEditViewer},'-',
   {label:T('lowQualityTop'),sc:'Ctrl+B',checked:state.lowQualityTop,run:toggleLowQualityTop},{label:T('nonConcordantTop'),checked:!!state.nonConcTop,run:toggleNonConcordantTop},'-',
   {label:T('sampleInfo'),sc:'Ctrl+F1',checked:state.tab==='info',run:()=>buildAfter(()=>showTab('info'))},{label:T('rawData'),sc:'Ctrl+F2',checked:state.tab==='raw',run:()=>buildAfter(()=>showTab('raw'))},{label:T('eptData'),sc:'Ctrl+F3',checked:state.tab==='ept',run:()=>buildAfter(()=>showTab('ept'))},'-',
   {label:T('fullView'),sc:'Ctrl+]',disabled:!['raw','ept'].includes(state.tab),run:()=>SampleViews.fullView()},
   {label:T('yAxis'),sub:[{label:T('scaleMaxY'),checked:!SampleViews.yRange().custom,disabled:state.tab!=='raw',run:()=>SampleViews.yScaleMax()},{label:T('scaleTo'),checked:SampleViews.yRange().custom,disabled:state.tab!=='raw',run:yScaleToDialog}]},'-',
   {label:T('binOffsets'),run:binOffsetsDialog,disabled:!state.filter?.run},'-',
   {label:T('showNavigator'),sc:'Ctrl+Shift+N',checked:state.showNavigator,run:toggleNavigator},'-',
   {label:T('language'),sub:[{label:'Русский',checked:lang==='ru',run:()=>setLang('ru')},{label:'English',checked:lang==='en',run:()=>setLang('en')}]}/*extra*/]},
  {label:T('tools'),items:[{label:T('manager'),sc:'Ctrl+M',run:()=>Methods.openManager()},{label:T('panelManager'),sc:'Ctrl+J',run:openPanelManager},{label:T('reportManager'),sc:'Ctrl+Q',run:openReportManager},'-',
   {label:T('tableSettingEditor'),sc:'Ctrl+T',run:editTableSetting},{label:T('codisManager'),run:()=>Profiles.codisManager()},{label:T('mixture'),sc:'Ctrl+U',run:()=>Mixture.open()},{label:T('mixtureManager'),sc:'Ctrl+Y',run:()=>Mixture.manager()},'-',
   {label:T('addProfile'),sub:[{label:T('pfCustom'),run:()=>addProfile('Custom Control')},{label:T('pfLab'),run:()=>addProfile('Lab Reference')}]},{label:T('profileManager'),run:()=>Profiles.profileManager()},{label:T('profileComparison'),run:()=>Profiles.profileComparison()},'-',
   {label:T('analysisMethodEditor'),run:()=>editMethod(state.project.samples.find(s=>state.selected.has(s.id))?.method)},{label:T('sizeStandardEditor'),run:editSizeStandard},{label:T('sizeMatchEditor'),run:openSizeMatch}]},
  {label:T('admin'),items:[{label:T('securityManager'),run:()=>Sec.securityManager(),disabled:!Sec.can('AdminToolAcc/General/May run application')},
   {label:T('auditManager'),sub:[{label:T('auditSetting'),run:()=>Sec.auditSettings(),disabled:!Sec.can('AuditGUI/Map Configuration Tool/May run application')},
    {label:T('auditReport'),run:()=>Sec.auditHistory(),disabled:!Sec.can('AuditGUI/Audit History Viewer/May run application')},'-',
    {label:T('scBackupMenu'),run:()=>Sec.auditBackup(),disabled:!Sec.can('AuditGUI/Audit Management/Backup Audit Records')},
    {label:T('scRestoreMenu'),run:()=>Sec.auditRestore(),disabled:!Sec.can('AuditGUI/Audit Management/Restore Audit Records')},{label:T('scDeleteMenu'),run:()=>Sec.auditDelete(),disabled:!Sec.can('AuditGUI/Audit Management/Delete Audit Records')},
    {label:T('scShowCount'),run:()=>Sec.auditCount()}]},
   {label:T('esig'),run:()=>Sec.esigAdmin(),disabled:!Sec.can('ESig/ESig Administration Tool/May run application')},'-',{label:T('scRemoteShutdown'),disabled:true}]},
  {label:T('help'),items:[{label:T('helpTopics'),sc:'F1',run:showHelp},{label:T('loadExample'),run:loadExample}/*extra*/,'-',{label:T('about'),run:()=>message(T('aboutText'),T('about'))}]},
 ]);
 const tb=$('toolbar');tb.replaceChildren(
  tbutton('newProject',T('newProject'),newProject),tbutton('openProject',T('openProject'),openProject,{id:'tbOpen'}),tbutton('saveProject',T('saveProject'),saveProject,{id:'tbSave'}),tsep(),
  tbutton('addSamples',T('addSamples'),addSamples),tbutton('exportTable',T('exportTable'),exportTable,{id:'tbExport'}),tsep(),
  tbutton('displayPlots',T('displayPlots'),displayPlots,{id:'tbPlots'}),tbutton('reportManager',T('reportManager'),openReportManager,{id:'tbReport'}),tbutton('labelEditViewer',T('labelEditViewer'),labelEditViewer,{id:'tbLabelEdits'}),
  tbutton('sizeMatchEditor',T('sizeMatchEditor'),openSizeMatch,{id:'tbSizeMatch'}),tsep(),tbutton('analysisMethodEditor',T('analysisMethodEditor'),()=>editMethod(state.project.samples.find(s=>state.selected.has(s.id))?.method),{id:'tbMethodEditor'}),tbutton('panelManager',T('panelManager'),openPanelManager,{id:'tbPanelManager'}),
  tbutton('manager',T('manager'),()=>Methods.openManager(),{id:'tbManager'}),tsep(),tbutton('analyze',T('analyze'),()=>analyze('pending'),{id:'tbAnalyze'}),tbutton('lowQualityTop',T('lowQualityTop'),()=>{state.lowQualityTop=!state.lowQualityTop;render()}),
  tsep(),el('span',{class:'tlabel',text:T('tableSetting')}));
 tb.append(tableSettingCombo(),tbutton('tableSettingEditor',T('tableSettingEditor'),editTableSetting,{id:'tbTableSetting'}),tsep(),tbutton('find',T('find'),findDialog,{id:'tbFind'}),tbutton('print',T('print'),printNow,{id:'tbPrint'}),
  tbutton('printPreview',T('printPreview'),printPreview,{id:'tbPrintPreview'}),tbutton('help',T('helpTopics'),showHelp));
 $('stopBtn').textContent=T('stop');
 renderTabs()}
function projectOptions(){ProjectOptions.open({methodSelect,panelSelect,sizeStdNames:()=>Object.keys(SIZE_STANDARDS),types:Object.entries(TYPE_KEYS).map(([k,v])=>[k,T(v)])})}
function buildAfter(fn){fn();buildChrome();render()}

$('stopBtn').addEventListener('click',()=>{state.stop=true});
function toggleNavigator(){state.showNavigator=!state.showNavigator;buildChrome();render()}
// Non-concordant Samples to Top: samples that share a profile ID with samples of another concordance group (ID-X flags them after Profile Concordance).
async function toggleNonConcordantTop(){if(state.nonConcTop){state.nonConcTop=false;buildChrome();render();return}
 const items=qcSamples().filter(x=>x.eligible&&x.profileId);if(!items.length)return message(T('pcNothingToCompare'));
 let r;try{r=await jsonPost('/api/profiles',{action:'compare',mode:'concordance',samples:items.map(x=>({id:x.id,markers:x.markers})),threshold:80,ignore:[]})}catch(e){return message(e.message)}
 const groupOf=new Map();for(const g of r.groups||[])for(const m of g.members)groupOf.set(m,g.id);
 const byProfile=new Map();for(const x of items){(byProfile.get(x.profileId)||byProfile.set(x.profileId,[]).get(x.profileId)).push(x)}
 const bad=new Set();for(const list of byProfile.values()){if(new Set(list.map(x=>groupOf.get(x.id)||x.id)).size>1)for(const x of list)bad.add(x.id)}
 const ids=new Set(state.project.samples.filter(s=>bad.has(s.file)).map(s=>s.id));state.nonConc=ids;state.nonConcTop=true;state.lowQualityTop=false;
 state.lastMessage=T('nonConcordantTop')+': '+ids.size;buildChrome();render()}
function toggleLowQualityTop(){state.lowQualityTop=!state.lowQualityTop;buildChrome();render()}
// Hot keys follow ID-X (docs: IDX_UI_HOTKEYS.md). Shift variants are separate bindings, never fall through to the plain key.
document.addEventListener('keydown',e=>{
 const k=hotKey(e);
 // Ctrl+D (Fill Down) must never reach the browser (bookmark), also while a table cell editor has the focus
 if(k==='d'&&(e.ctrlKey||e.metaKey)&&!e.shiftKey&&!e.altKey){e.preventDefault();if(!e.target.closest('.dlg,.back,textarea'))fillDown();return}
 if(e.target.closest('input,select,textarea'))return;
 if(e.key==='Delete'&&!e.ctrlKey&&!e.metaKey&&!e.shiftKey&&!e.altKey&&!e.target.closest('.dlg,.back')&&!document.querySelector('.dlg')){e.preventDefault();clearCells();return}
 if(e.key==='F1'&&!e.ctrlKey&&!e.metaKey){e.preventDefault();showHelp();return}
 if(!e.ctrlKey&&!e.metaKey)return;
 if(/^F[123]$/.test(e.key)&&!e.shiftKey&&!e.altKey){e.preventDefault();buildAfter(()=>showTab({F1:'info',F2:'raw',F3:'ept'}[e.key]));return}
 if(e.code==='BracketRight'&&!e.shiftKey&&!e.altKey){e.preventDefault();if(['raw','ept'].includes(state.tab))SampleViews.fullView();return}
 if(e.shiftKey){
  const sm={Digit1:()=>buildAfter(()=>showTab('samples')),Digit2:()=>buildAfter(()=>showTab('summary')),Digit3:()=>buildAfter(()=>showTab('genotypes')),KeyR:()=>analyze('all'),KeyN:toggleNavigator,KeyZ:redoProject};
  if(sm[e.code]){e.preventDefault();sm[e.code]()}return}
 const map={z:undoProject,i:fitToContent,u:()=>Mixture.open(),y:()=>Mixture.manager(),o:openProject,s:saveProject,d:fillDown,c:copyRows,v:pasteCells,t:editTableSetting,f:findDialog,g:sortDialog,m:()=>Methods.openManager(),j:openPanelManager,n:newProject,k:addSamples,e:exportTable,l:displayPlots,q:openReportManager,p:printNow,r:()=>analyze('pending'),a:selectAll,b:toggleLowQualityTop};
 if(map[k]&&!(k==='z'&&/INPUT|TEXTAREA|SELECT/.test(e.target.tagName))){e.preventDefault();map[k]()}});
window.addEventListener('beforeunload',e=>{if(state.project.dirty&&state.project.samples.length){e.preventDefault();e.returnValue=''}});

function qcSamples(){return state.project.samples.map(s=>{const eff=s.report?effective(s):null;const markers={},dye={};
 if(eff)for(const c of eff.calls){if(!c.label)continue;(markers[c.marker]=markers[c.marker]||[]).push(String(c.label));dye[c.marker]=dyeLetter(c.channel)}
 const q=samplePqv(s).SQ;const ol=Object.values(markers).some(a=>a.some(x=>/^OL$/i.test(x)));const ud=s.ud||{};const info=s.analysis?.run_info||{};
 return {id:s.file,file:s.file,name:s.name,type:s.type,analyzed:!!s.report,profileId:(s.profileId||'').trim(),markers,dye,hasOl:ol,well:info.well||'',runDate:info.run_date||'',panelMarkers:(sampleKit(s)?.markers||[]).map(m=>m.name),category:s.specimenCategory||'No Export',
  eligible:!!s.report&&s.type!=='ladder'&&['pass','check','override'].includes(q)&&!ol&&Object.keys(markers).length>0,
  codis:{sample_name:s.name,specimen_id:ud.UD1||s.name,category:s.specimenCategory||'No Export',panel:s.panel||'',batch_id:ud.UD2||info.plate||'',case_id:ud.UD3||'',
   comment:ud.UD4||'',partial:ud.UD5||'',source_id:ud.UD6||'',ncic:ud.UD7||'',vicap:ud.UD8||'',loci:Object.entries(markers).map(([marker,alleles])=>({marker,alleles,partial_locus:!!eff?.pli.has(marker)}))}}})}
function addProfile(type){if(!Sec.need('UPDATE_PROFILE_CONCORDANCE'))return;const list=qcSamples().filter(x=>state.selected.has(state.project.samples.find(s=>s.file===x.file)?.id));
 if(!list.length)return message(T('selectSamplesFirst'));Profiles.addFromSamples(list,type);render()}
Profiles.init({post:jsonPost,samples:qcSamples,plot:files=>reportApi.plot(files),user:()=>Sec.user?.name||'',
 setUser:u=>{try{localStorage.setItem('opentrace.user',u)}catch(e){}}});
window.addEventListener('opentrace-profiles',()=>render());window.addEventListener('opentrace-codis',()=>{Profiles.codisCatalog().then(()=>render()).catch(()=>{})});
Profiles.codisCatalog().then(()=>render()).catch(()=>{});
function mixSamples(){return state.project.samples.map(s=>{const eff=s.report?effective(s):null;const kit=sampleKit(s);
 const y=new Set((kit?.markers||[]).filter(m=>m.y_marker).map(m=>m.name));const byM=new Map();
 if(eff)for(const c of [...eff.calls].sort((a,b)=>a.size_bp-b.size_bp)){if(!c.label)continue;if(!byM.has(c.marker))byM.set(c.marker,[]);byM.get(c.marker).push({label:String(c.label),height:c.height})}
 const q=samplePqv(s).SQ;const markers=[...byM].map(([name,alleles])=>({name,y:y.has(name),alleles}));
 return {key:s.runFolder+'/'+s.file,file:s.file,name:s.name,runFolder:s.runFolder,panel:s.panel||'',markers,
  eligibleBase:!!eff&&s.type==='sample'&&['pass','check','override'].includes(q)&&markers.length>0,ol:markers.some(m=>m.alleles.some(a=>a.label.toUpperCase()==='OL'))}})}
Mixture.init({post:jsonPost,samples:mixSamples,plot:keys=>{const set=new Set(keys);state.selected=new Set(state.project.samples.filter(s=>set.has(s.runFolder+'/'+s.file)).map(s=>s.id));render();displayPlots()},results:()=>JSON.parse(JSON.stringify(state.project.mixture||{})),
 saveResults:r=>{state.project.mixture=r;state.project.dirty=true;render()},knowns:()=>Profiles.profiles()});
// Info / Raw Data / EPT Data tabs: selected samples in table order; settings from the analysis method.
function sampleExtra(s){const m=Methods.get(s.method)||{};const p=m.peak_detector||{};const ar=p.analysis_range||{},sr=p.sizing_range||{};
 const th=p.thresholds?Object.entries(p.thresholds).map(([k,v])=>`${T('dyeLetter_'+k)} ${v}`).join(', '):'';
 const errors=[...arnmReasons(s)];if(s.analysis?.sizing&&s.analysis.sizing.status&&s.analysis.sizing.status!=='accepted')errors.push(`${T('siSizing')}: ${s.analysis.sizing.status}`);
 return {status:STATUS_TEXT[s.status]||s.status,sampleType:TYPE_TEXT[s.type]||s.type,errors,settings:[
  [T('si_sampleType'),TYPE_TEXT[s.type]||s.type],[T('colPanel'),s.panel||''],[T('colSizeStandard'),s.sizeStd],[T('si_matrix'),s.inst?.is310?(s.matrix||(s.inst.embedded?T('siMatrixInFile'):'—')):T('siNoMatrix')],
  [T('colAnalysisMethod'),s.method],[T('si_sizeCalling'),p.size_calling||''],[T('si_analysisRange'),ar.full?T('amFullRange'):`${ar.start} – ${ar.stop}`],
  [T('si_sizingRange'),sr.all?T('amAllSizes'):`${sr.start_bp} – ${sr.stop_bp}`],[T('si_baseline'),p.baseline_window??''],[T('si_smoothing'),p.smoothing??''],
  [T('si_thresholds'),th],[T('si_polyDegree'),p.polynomial_degree??''],[T('si_windowSize'),p.peak_window??''],[T('si_minHalfWidth'),p.min_half_width??''],
  [T('si_slopes'),p.slope_start!=null?`${p.slope_start} / ${p.slope_end}`:''],[T('si_analysisVersion'),s.analysis?.analysis_version||'—']]}}
SampleViews.init({samples:()=>sortedSamples().filter(s=>state.selected.has(s.id)),extra:sampleExtra});
Methods.init(jsonPost);
// ---------- Manager > Projects tab: Open, Save As, Rename, Delete, Export, Import (ID-X ProjectActor / ProjectExporter / ProjectImporter) ----------
let projectRows=[];
const projFmt=(k,...a)=>T(k).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
function askName(title,value){return new Promise(res=>{dialog(title,b=>{b.append(el('div',{class:'field'},el('label',{text:T('amName')}),el('input',{type:'text',id:'pnName',value})))},
 [[T('ok'),b=>{const v=b.querySelector('#pnName').value.trim();if(!v)return false;res(v)},true],[T('cancel'),()=>{res(null)}]]).root.querySelector('.x').addEventListener('click',()=>res(null))})}
// analysis settings carried by a project package: ID-X imports them unless the user lacks the Update right of that kind
function mergeSettings(snap,mode){
 const out={added:[],overwritten:[],skipped:[],denied:[]};const can=f=>!window.Sec||Sec.can(f);
 const kinds=[['methods',T('mgrMethods'),'UPDATE_ANALYSIS_METHOD',m=>m.name,n=>Methods.get(n),m=>Methods.put(m)],
  ['sizeStds',T('mgrSizeStandards'),'UPDATE_SIZE_STANDARD',m=>m.name,n=>SizeStd.get(n),m=>SizeStd.put(m)],
  ['matrices',T('mgrMatrices'),'UPDATE_MATRIX',m=>m.name,n=>Matrices.get(n),m=>Matrices.put(m)],
  ['kits',T('panelMgrTitle'),'UPDATE_PANEL',k=>k.name,n=>KitStore.load().kits.find(k=>k.name===n),k=>{const db=KitStore.load();const i=db.kits.findIndex(x=>x.name===k.name);if(i>=0)db.kits[i]=k;else db.kits.push(k);KitStore.save(db)}]];
 for(const [key,label,right,name,get,put] of kinds){const list=snap[key]||[];if(!list.length)continue;
  if(!can(right)){out.denied.push(label);continue}
  for(const o of list){const n=name(o);const has=!!get(n);if(has&&mode!=='overwrite'){out.skipped.push(`${label}: ${n}`);continue}
   try{put(o);(has?out.overwritten:out.added).push(`${label}: ${n}`)}catch(e){out.skipped.push(`${label}: ${n} (${e.message})`)}}}
 if(out.overwritten.length){kitCache.clear();settingsCache.clear()}
 return out}
function exportProjectsDialog(names){
 if(!names.length)return message(T('selectRow'));
 if(state.project.dirty&&names.includes(state.project.savedAs))return message(projFmt('peInUse',state.project.savedAs));
 dialog(T('peTitle'),b=>{b.classList.add('pexdlg');const cb=(id,label,on)=>el('label',{class:'chk'},el('input',{type:'checkbox',id,...(on?{checked:''}:{})}),document.createTextNode(' '+label));
  b.append(el('p',{text:names.join(', ')}),cb('peAuto',T('peAutoName'),true),cb('peSettings',T('peWithSettings'),true),cb('peFsa',T('peSampleFiles'),false),el('p',{class:'note',text:T('peNote')}))},
  [[T('mgrExport'),async b=>{const auto=b.querySelector('#peAuto').checked,settings=b.querySelector('#peSettings').checked,files=b.querySelector('#peFsa').checked;
    for(const n of names){let file=n;if(!auto){file=await askName(projFmt('peExportOne',n),n);if(!file)continue}
     $('statusText').textContent=projFmt('peExporting',n);
     try{const r=await jsonPost('/api/idx-project',{action:'export',name:n,settings,sample_files:files});
      Methods.download(`${file.replace(/[\\/:*?"<>|]+/g,'_')}.otproj`,b64ToBlob(r.package_b64,file+'.otproj'),'application/zip');
      Sec.record([{map:'Project',attribute:'exported',instance:n,old:'',new:file+'.otproj',event:'Export Project'}])}catch(e){message(T('peProblem')+'\n'+e.message);return}}
    $('statusText').textContent=projFmt('peDone',names.length)},true],[T('cancel'),()=>true]])}
function importProjectDialog(refresh){
 if(!Sec.need('UPDATE_PROJECT'))return;let file=null;
 dialog(T('piTitle'),b=>{b.classList.add('pimdlg');const inp=el('input',{type:'file',accept:'.otproj,.zip',style:'display:none'});const lbl=el('span',{id:'piFile',text:T('mxNoFile')});
  inp.addEventListener('change',()=>{file=inp.files[0]||null;lbl.textContent=file?file.name:T('mxNoFile')});
  const choose=el('button',{class:'btn',type:'button',id:'piChoose',text:T('piChoose')});choose.addEventListener('click',()=>inp.click());
  const rb=(v,label,on)=>el('label',{class:'chk'},el('input',{type:'radio',name:'piMode',value:v,...(on?{checked:''}:{})}),document.createTextNode(' '+label));
  b.append(el('div',{style:'display:flex;gap:8px;align-items:center'},choose,lbl,inp),el('fieldset',{},el('legend',{text:T('piSettings')}),el('div',{text:T('piSameName')}),rb('skip',T('piSkip'),true),rb('overwrite',T('piOverwrite'))))},
  [[T('mgrImport'),async b=>{if(!file){message(T('selectRow'));return false}const mode=b.querySelector('input[name=piMode]:checked').value;
    if(mode==='overwrite'&&!(await Methods.ask(T('piConfirm'))))return false;
    let pkg;try{pkg=await blobToB64(file)}catch(e){message(e.message);return false}
    let newName=null;
    for(;;){$('statusText').textContent=projFmt('piImporting',file.name);
     try{const r=await jsonPost('/api/idx-project',{action:'import',package_b64:pkg,mode,new_name:newName});
      const m=mergeSettings(r.snapshots||{},mode);
      Sec.record([{map:'Project',attribute:'created',instance:r.name,old:'',new:file.name,event:'Import Project'}]);
      const lines=[projFmt('piDone',r.name,r.samples)];
      if(!r.sample_files&&r.samples)lines.push(T('piNoFiles'));else if(r.sample_files<r.samples)lines.push(projFmt('piSomeFiles',r.sample_files,r.samples));
      if(r.status_changed)lines.push(projFmt('piChanged',r.status_changed));
      if(m.added.length)lines.push(T('piAdded')+'\n  '+m.added.join('\n  '));
      if(m.overwritten.length)lines.push(T('piOverwritten')+'\n  '+m.overwritten.join('\n  '));
      if(m.skipped.length)lines.push(T('piSkipped')+'\n  '+m.skipped.join('\n  '));
      for(const k of m.denied)lines.push(projFmt('piDenied',k));
      message(lines.join('\n\n'),T('piTitle'));$('statusText').textContent=lines[0];await refresh();return true}
     catch(e){if(/already been used/.test(e.message)){newName=await askName(projFmt('piNameUsed',newName||file.name.replace(/\.[^.]+$/,'')),(newName||'')+' (2)');if(!newName)return true;continue}
      message(T('piProblem')+'\n'+e.message);return false}}}],[T('cancel'),()=>true]])}
Methods.registerKind(()=>({id:'projects',first:true,multi:true,label:T('mgrProjects'),heads:[T('colOwner'),T('colSampleCount')],
 rows:()=>projectRows.map(p=>({name:p.name,saved:p.modified,a:p.owner||'',b:String(p.samples)})),group:r=>projectRows.find(p=>p.name===r.name)?.security_group||'',
 load:async()=>{projectRows=(await jsonPost('/api/idx-project',{action:'list'})).projects},
 open:async(name,{close})=>{if(!Sec.need('READ_PROJECT'))return;if(!(await confirmDiscard()))return;close();await loadProject(name)},
 buttons:({btn,selected,refresh,close})=>{const one=()=>{const s=selected();if(s.length!==1){message(T('selectRow'));return null}return s[0]};
  return [btn(T('mgrOpen'),async()=>{const n=one();if(!n||!Sec.need('READ_PROJECT'))return;if(!(await confirmDiscard()))return;close();await loadProject(n)},'prjOpenBtn'),
   btn(T('mgrSaveAs'),async()=>{const n=one();if(!n||!Sec.need('UPDATE_PROJECT'))return;const v=await askName(projFmt('prjSaveAs',n),n+' copy');if(!v)return;
    try{await jsonPost('/api/idx-project',{action:'save_as',name:n,new_name:v});Sec.record([{map:'Project',attribute:'created',instance:v,old:n,new:v,event:'Save Project As'}]);await refresh()}catch(e){message(e.message)}},'prjSaveAsBtn'),
   btn(T('mgrRename'),async()=>{const n=one();if(!n||!Sec.need('UPDATE_PROJECT'))return;if(n===state.project.savedAs)return message(projFmt('prjIsOpen',n));const v=await askName(projFmt('prjRename',n),n);if(!v||v===n)return;
    try{await jsonPost('/api/idx-project',{action:'rename',name:n,new_name:v});Sec.record([{map:'Project',attribute:'name',instance:v,old:n,new:v,event:'Rename Project'}]);await refresh()}catch(e){message(e.message)}},'prjRenameBtn'),
   btn(T('mgrImport'),()=>importProjectDialog(refresh),'prjImportBtn'),
   btn(T('mgrExport'),()=>{const s=selected();if(!s.length||!Sec.need('READ_PROJECT'))return message(T('selectRow'));exportProjectsDialog(s)},'prjExportBtn'),
   btn(T('mgrDelete'),async()=>{const s=selected();if(!s.length)return message(T('selectRow'));if(!Sec.need('UPDATE_PROJECT'))return;
    for(const n of s){if(n===state.project.savedAs){message(projFmt('prjIsOpen',n));continue}if(!(await Methods.ask(T('confirmDeleteProject').replace('{0}',n))))continue;
     const ch=await Sec.change({map:'Project',attribute:'deleted',instance:n,old:n,new:'',event:'Delete Project',esig:{group:'GM_PROJECT',action:'DELETE_PROJECT'}});if(!ch)continue;
     try{await jsonPost('/api/idx-project',{action:'delete',name:n});await ch.commit()}catch(e){message(e.message)}}await refresh()},'prjDeleteBtn')]}}));
buildChrome();render();
window.addEventListener('opentrace-security',()=>{buildChrome();render()});
Sec.init().then(ok=>{if(ok===false)return;buildChrome();render();
 if(window.TableSettings&&TableSettings.ensureShipped)TableSettings.ensureShipped().then(()=>render());
 Methods.ensureDefault().then(()=>{render();
  let last=null;try{last=localStorage.getItem('opentrace.lastProject')}catch(e){}
  if(ProjectOptions.get().startup==='last'&&last&&Sec.can('READ_PROJECT'))return loadProject(last)}).catch(e=>message(e.message))});
})();
