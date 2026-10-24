// OpenTrace Project window: samples, analysis summary and genotypes.
(function(){
const $=id=>document.getElementById(id);
const TOKEN=window.OT_TOKEN;

// Size standards come from the library (sizestd.js); this view maps name -> fragment list.
const SIZE_STANDARDS=new Proxy({},{get:(_,name)=>SizeStd.get(String(name))?.sizes,has:(_,name)=>!!SizeStd.get(String(name)),
 ownKeys:()=>SizeStd.names(),getOwnPropertyDescriptor:(_,name)=>SizeStd.get(String(name))?{enumerable:true,configurable:true,value:SizeStd.get(String(name)).sizes}:undefined});
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
window.OT={state,plotPayload,printPayload:()=>printPayload(),report:null,edit:(id,spec)=>applyEdit(id,spec),editItems:(id,spec)=>editItems(id,spec),labelEditViewer:()=>labelEditViewer(),kitsChanged:()=>{dropCaches();render()}};
window.addEventListener('opentrace-options',()=>render());
window.addEventListener('opentrace-tables',()=>{state.sort=null;buildChrome();render()});
window.addEventListener('opentrace-methods',()=>{dropCaches();
 for(const s of state.project.samples)if(s.method!==EXAMPLE_METHOD&&!Methods.get(s.method))s.method=defaultMethod();render()});
const panelChoices=()=>KitStore.choices(KitStore.load());
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
async function analyzeSettings(methodName,sizeStd){
 const key=methodName+'\u0001'+sizeStd;if(settingsCache.has(key))return settingsCache.get(key);
 const method=Methods.get(methodName);if(!method)throw Error(T('methodMissing')+' '+methodName);
 const std=SizeStd.get(sizeStd);if(!std)throw Error(T('arnmStandard')+': '+sizeStd);
 const r=await jsonPost('/api/analysis-method',{action:'analyze_settings',method,standard_bp:std.sizes,standard_dye:std.dye});
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
function editMethod(name){const m=Methods.get(name||defaultMethod());if(!m)return message(T('methodMissing')+' '+name);if(!Sec.need('READ_ANALYSIS_METHOD'))return;
 const restrict=Sec.can('UPDATE_ANALYSIS_METHOD')?null:(Sec.can('UPDATE_ANALYSIS_RANGE')||Sec.can('UPDATE_SIZING_RANGE'))?'ranges':'read';
 Methods.openEditor(m,{restrict,onSave:(mm,prev)=>{Methods.put(mm,prev);for(const s of state.project.samples)if(s.method===prev){s.method=mm.name;if(s.status==='analyzed')s.status='changed'}
  state.project.dirty=true;render()}})}
function panelSelect(value,onChange){const sel=el('select');sel.append(new Option(T('noPanelChoice'),''));
 const ch=panelChoices();for(const c of ch){const o=new Option(c.label,c.key);o.title=`${c.kit} / ${c.binSets.join(', ')}`;sel.append(o)}
 if(value===EXAMPLE_KEY)sel.append(new Option(state.kit?.name||'example',EXAMPLE_KEY));
 sel.value=value||'';sel.addEventListener('change',()=>onChange(sel.value));sel.addEventListener('mousedown',e=>e.stopPropagation());return sel}

async function jsonPost(url,data){
 const r=await fetch(url,{method:'POST',headers:{'X-Session-Token':TOKEN,'X-User-Ticket':Sec.ticket(),'Content-Type':'application/json'},body:JSON.stringify(data)});
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
// ---------- table cells (columns chosen by the active Table Setting) ----------
const PQV_NUM={pass:0,check:1,low:2,na:-2,override:0.5};
function arnmCodes(s){const r=[];
 if(!s.blob&&!s.analysis)r.push('arsSfnf');
 if(s.method!==EXAMPLE_METHOD&&!Methods.get(s.method))r.push('arnmMethod');
 if(!SIZE_STANDARDS[s.sizeStd]&&s.method!==EXAMPLE_METHOD)r.push('arnmStandard');
 if(!s.panelKey)r.push('arnmPanel');
 if(s.type!=='ladder'&&s.panelKey&&!state.project.samples.some(x=>x.runFolder===s.runFolder&&x.type==='ladder'&&x.panelKey===s.panelKey))r.push('arnmLadder');
 return r}
const arnmReasons=s=>arnmCodes(s).map(k=>T(k));
// Requirements without which a sample cannot be analyzed at all (ID-X: non-optional requirements).
const BLOCKING=new Set(['arsSfnf','arnmMethod','arnmStandard']);
const blocked=s=>arnmCodes(s).some(k=>BLOCKING.has(k));
// MIX PQV (ID-X help): two or more markers with three or more alleles, or one such marker and two or more
// markers with a PHR flag, or any edited marker -> Check; ladders and samples without labels -> NA.
function mixFlag(s,eff){if(s.type==='ladder'||!eff||!eff.calls.length)return 'na';
 const per={};for(const c of eff.calls)if(c.label)per[c.marker]=(per[c.marker]||0)+1;
 const many=Object.values(per).filter(n=>n>=3).length;const phr=genoRows(s).filter(r=>r.flags.PHR==='check').length;
 return many>=2||(many>=1&&phr>=2)||activeEdits(s).length?'check':'pass'}
function sampleFlags(s){const q=samplePqv(s);
 const eff=s.report?effective(s):null;if(eff?.cgqOverride)q.CGQ='override';if(s.report&&q.SQ!=='low')q.MIX=mixFlag(s,eff);
 return {...q,SE:s.report?(activeEdits(s).length?'check':'pass'):'na',SQO:s.analysis?.sizing?.sq_override?'check':'na',MNF:'na',SFNF:(s.blob||s.analysis)?'pass':'low',SNF:(SIZE_STANDARDS[s.sizeStd]||s.method===EXAMPLE_METHOD)?'pass':'low',
  ARNM:arnmReasons(s).length?'low':'pass'}}
const changed=s=>{if(s.status==='analyzed')s.status='changed';state.project.dirty=true;render()};
// Inline text cell (Sample ID, Comment, Profile ID, UD1-UD9): edits mark the project dirty.
function setField(s,key,v){if(!Sec.can('UPDATE_PROJECT')){Sec.denied();render();return}if(key.startsWith('ud.')){s.ud=s.ud||{};s.ud[key.slice(3)]=v}else s[key]=v;state.project.dirty=true}
function textEdit(s,key,value){const i=el('input',{type:'text',value:String(value),'data-field':key});
 i.addEventListener('mousedown',e=>e.stopPropagation());i.addEventListener('focus',()=>{state.lastField=key});
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
   sel.addEventListener('change',()=>{s.type=sel.value;changed(s)});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:T(TYPE_KEYS[s.type]),node:sel}}
  case 'analysisMethod':{const sel=methodSelect(s.method,v=>{s.method=v;try{localStorage.setItem('opentrace.lastMethod',v)}catch(e){}changed(s)});
   return {value:s.method,node:sel,dbl:()=>{if(s.method!==EXAMPLE_METHOD)editMethod(s.method)}}}
  case 'panel':{const sel=panelSelect(s.panelKey,v=>{s.panelKey=v;s.panel=v?v.split('\u0001')[1]:'';if(v)s.sizeStd=fitStandard(v,s.sizeStd);changed(s)});
   return {value:s.panel||'',node:sel}}
  case 'sizeStandard':{const sel=el('select');for(const k of Object.keys(SIZE_STANDARDS))sel.append(new Option(k,k));sel.value=s.sizeStd;
   sel.addEventListener('change',()=>{s.sizeStd=sel.value;changed(s)});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:s.sizeStd,node:sel}}
  case 'matrix':return {value:''};
  case 'runName':return {value:info.run_name||''};
  case 'instrumentType':return {value:info.instrument_type||''};
  case 'instrumentID':return {value:info.instrument_id||''};
  case 'runDate':return {value:info.run_date||''};
  case 'plate':return {value:info.plate||''};
  case 'well':return {value:info.well||''};
  case 'lane':return {value:info.capillary??'',cls:'num'};
  case 'sampleFileNorm':return {value:''};
  case 'customControl':{const sel=el('select');sel.append(new Option('',''));for(const p of Profiles.customControls())sel.append(new Option(p.id,p.id));
   if(s.customControl&&!Profiles.getProfile(s.customControl))sel.append(new Option(s.customControl,s.customControl));sel.value=s.customControl||'';
   sel.addEventListener('change',()=>{s.customControl=sel.value;changed(s)});sel.addEventListener('mousedown',e=>e.stopPropagation());return {value:s.customControl||'',node:sel}}
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
function effective(s){
 const rep=s.report;const calls=(rep?.calls||[]).map(c=>({...c}));const byId=new Map(calls.map(c=>[c.id,c]));
 const out={calls,edited:new Set(),gqOverride:new Set(),cgqOverride:false,reasons:{}};
 for(const e of activeEdits(s)){
  if(e.action==='override_cgq'){out.cgqOverride=true;continue}
  out.edited.add(e.marker);out.reasons[e.marker]=e.reason;
  if(e.action==='override_gq')out.gqOverride.add(e.marker);
  else if(e.action==='rename'){const c=byId.get(e.callId);if(c){c.label=e.new;c.user_edited=true}}
  else if(e.action==='delete'){const i=calls.findIndex(c=>c.id===e.callId);if(i>=0)calls.splice(i,1)}
  else if(e.action==='add'){const c={...e.peak,id:'u'+e.id,marker:e.marker,label:e.new,flags:['user_added'],user_edited:true};calls.push(c);byId.set(c.id,c)}}
 calls.sort((a,b)=>a.size_bp-b.size_bp);return out}
// Audit items for one label edit (map Allele / Sample of the ID-X audit map).
function editItems(sampleId,spec){const s=state.project.samples.find(x=>x.id===sampleId);if(!s||!s.report)return [];
 const eff=effective(s);const old=spec.callId?eff.calls.find(c=>c.id===spec.callId):null;const inst=`${s.runFolder}/${s.file}`;
 const size=old?.size_bp??spec.peak?.size_bp;const where=`${inst} · ${spec.marker||''}${size!=null?' · '+Number(size).toFixed(2)+' bp':''}`;
 if(spec.action==='override_gq')return [{map:'Sample',attribute:'OverrideGQ',instance:`${inst} · ${spec.marker}`,old:'',new:'1.0',label:`${T('overrideGq')} (${spec.marker})`,event:'Override GQ'}];
 if(spec.action==='override_cgq')return [{map:'Sample',attribute:'OverrideCGQ',instance:inst,old:'',new:'1.0',label:T('overrideCgq'),event:'Override CGQ'}];
 const attr={add:'created',rename:'modified',delete:'deleted'}[spec.action];
 return [{map:'Allele',attribute:attr,instance:where,old:old?String(old.label):'',new:spec.action==='delete'?'':String(spec.new??''),label:`${T('colAllele')} ${spec.marker||''}`,
  event:{created:'Add Allele',modified:'Modify Allele',deleted:'Delete Allele'}[attr]}]}
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
 const user=Sec.user?.name||'';
 const eff=effective(s);const old=spec.callId?eff.calls.find(c=>c.id===spec.callId):null;
 s.edits=s.edits||[];const id=(s.edits.reduce((m,e)=>Math.max(m,e.id),0)||0)+1;
 s.edits.push({id,at:new Date().toISOString(),user,reason,action:spec.action,marker:spec.marker||null,callId:spec.callId||null,
  peak:spec.peak||null,old:old?old.label:null,new:spec.action==='delete'?null:label??null,size_bp:old?.size_bp??spec.peak?.size_bp??null});
 s.rev=(s.rev||0)+1;state.project.dirty=true;render();
 await Sec.record(items.map(x=>({...x,reason})));return true}
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
 return (rep.allele_calling.markers||[]).map(m=>{const calls=eff.calls.filter(c=>c.marker===m.name);
  const g=gq[m.name];const metrics=g?.score?.metrics||{};
  const f=k=>metrics[k]===1?'check':metrics[k]===0?'pass':'na';
  const flags={OS:f('offscale'),BIN:f('binning'),PHR:f('ratio'),LPH:f('absolute_height'),MPH:f('high_height'),SPK:f('spike'),
   AN:f('allele_number'),BD:f('broad'),OVL:f('overlap'),ACC:'na',CC:ccFlag(rep,m.name),MEC:'na',ADO:'na',ME:eff.edited.has(m.name)?'check':'pass',
   GQ:eff.gqOverride.has(m.name)?'override':g?code(g.category?.code):'na'};
  return {sample:s,marker:m.name,channel:m.channel,calls,gq:eff.gqOverride.has(m.name)?1:g?.score?.gq,flags,peaks:rep.peaks||[],reason:eff.reasons[m.name]||''}})}
function genoCell(id,r,i){
 const s=r.sample;const c=r.calls[i]??(i===1&&r.calls.length===1&&ProjectOptions.get().dupHomo&&autosomal(r.marker)?r.calls[0]:undefined);const info=s.analysis?.run_info||{};
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
  case 'PLI':return {value:''};
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
function tableHead(table,cols){
 const t=el('table',{class:'g'});const cg=el('colgroup');cg.append(el('col',{style:'width:28px'}));
 for(const c of cols)cg.append(el('col',{style:`width:${c.width}px`}));t.append(cg);
 const hr=el('tr');hr.append(el('th',{text:''}));const srt=currentSort(table);
 for(const c of cols){const k=srt.findIndex(x=>x.col===c.base);const th=el('th',{text:c.head+(k===0?(srt[0].desc?' ▼':' ▲'):''),title:c.title||c.head,'data-col':c.base});
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
function sortedSamples(){const list=visibleSamples();if(state.lowQualityTop)return list;
 const flags=new Map(list.map(s=>[s.id,sampleFlags(s)]));return sortRows(list,currentSort('samples'),(id,s)=>sampleCell(id,s,flags.get(s.id)))}
function sortedGenoRows(){const rows=visibleSamples().flatMap(genoRows);return sortRows(rows,currentSort('genotypes'),(id,r)=>genoCell(id,r,0))}
function renderSamples(){
 const grid=$('samplesGrid');grid.replaceChildren();const cols=sampleColumns();const t=tableHead('samples',cols);
 const tb=el('tbody');const list=sortedSamples();
 list.forEach((s,i)=>{const flags=sampleFlags(s);
  const tr=el('tr',{class:state.selected.has(s.id)?'sel':''});tr.dataset.id=s.id;tr.append(el('td',{class:'rh',text:String(i+1)}));
  for(const c of cols){const v=sampleCell(c.base,s,flags);const td=el('td');fillCell(td,v);if(v.dbl)td.addEventListener('dblclick',e=>{e.stopPropagation();v.dbl()});tr.append(td)}
  tr.addEventListener('mousedown',e=>selectRow(e,s.id,list.map(x=>x.id)));
  tr.addEventListener('dblclick',()=>displayPlots());tb.append(tr)});
 t.append(tb);grid.append(t);
 if(!list.length)grid.append(el('div',{class:'empty',text:state.project.name?'':T('untitled')}))}
function selectRow(e,id,order){
 if(e.shiftKey&&state.anchor!=null){const a=order.indexOf(state.anchor),b=order.indexOf(id);const [lo,hi]=a<b?[a,b]:[b,a];
  state.selected=new Set(order.slice(lo,hi+1))}
 else if(e.ctrlKey||e.metaKey){state.selected.has(id)?state.selected.delete(id):state.selected.add(id);state.anchor=id}
 else{state.selected=new Set([id]);state.anchor=id}
 render();refreshPlots()}
function refreshPlots(){if(!ProjectOptions.get().updatePlots)return;try{const w=state.plotWin;if(w&&!w.closed&&w.otRefresh)w.otRefresh()}catch(e){}}

// ---------- genotypes table ----------
function renderGenotypes(){
 const grid=$('genotypesGrid');grid.replaceChildren();const cols=genoColumns();const t=tableHead('genotypes',cols);
 const tb=el('tbody');const rows=sortedGenoRows();const order=[...new Set(rows.map(r=>r.sample.id))];
 rows.forEach((r,n)=>{const tr=el('tr',{class:state.selected.has(r.sample.id)?'sel':''});tr.append(el('td',{class:'rh',text:String(n+1)}));
  for(const c of cols){const td=el('td');fillCell(td,genoCell(c.base,r,c.index||0));tr.append(td)}
  if(r.calls.length>TableSettings.active().genotypes.numAlleles)tr.title=T('moreAlleles');
  tr.addEventListener('mousedown',e=>selectRow(e,r.sample.id,order));tr.addEventListener('dblclick',()=>displayPlots());tb.append(tr)});
 t.append(tb);grid.append(t)}
function dyeLetter(tag){const i={DATA1:'B',DATA2:'G',DATA3:'Y',DATA4:'R'}[tag];return i||(tag==='DATA105'?'P':tag==='DATA106'?'O':'')}

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
 bar.hidden=!(f&&f.ids);bar.replaceChildren();if(bar.hidden)return;
 const all=el('a',{text:T('showAllSamples'),id:'clearFilter'});all.addEventListener('click',()=>{state.filter=null;render()});
 bar.append(document.createTextNode(`${T('filtered')}: ${f.label||''} (${f.ids.size}) `),all)}

// ---------- render ----------
function renderTitle(){document.title=`${T('app')}${Sec.user&&Sec.state.require_login?' ['+Sec.user.name+']':''} - ${state.project.name?(state.project.dirty?'*':'')+state.project.name:T('untitled')}`}
function render(){
 document.title=`${T('app')} - ${state.project.name?(state.project.dirty?'*':'')+state.project.name:T('untitled')}`;
 renderNavigator();
 renderFilterBar();
 if(state.tab==='samples')renderSamples();else if(state.tab==='summary')renderSummary();else renderGenotypes();
 const any=state.project.samples.length>0;
 for(const id of ['tbAnalyze','tbPlots','tbExport'])if($(id))$(id).disabled=!any||state.busy;
 $('stopBtn').disabled=!state.busy;
 if(!state.busy)$('statusText').textContent=state.lastMessage||`${state.project.samples.length} ${T('samples').toLowerCase()}`;
}
function progress(done,total,text){const pct=total?Math.round(100*done/total):0;$('progressBar').style.width=pct+'%';$('progressText').textContent=pct+'%';if(text)$('statusText').textContent=text}

// ---------- actions ----------
const notYet=()=>message(T('notImplemented'));
// ---------- project library (server folder) ----------
const sid=s=>'s'+s.id;
function blobToB64(blob){return new Promise((res,rej)=>{const r=new FileReader();r.onload=()=>res(String(r.result).split(',')[1]);r.onerror=rej;r.readAsDataURL(blob)})}
function b64ToBlob(b64,name){const bin=atob(b64);const a=new Uint8Array(bin.length);for(let i=0;i<bin.length;i++)a[i]=bin.charCodeAt(i);return new File([a],name)}
function projectMeta(){const used=new Set(state.project.samples.map(s=>s.method));const kitsUsed=new Set(state.project.samples.map(s=>(s.panelKey||'').split('\u0001')[0]).filter(Boolean));
 return {comment:state.project.comment||'',run_folders:state.project.runFolders,reports:state.project.reports||[],mixture:state.project.mixture||{},
  samples:state.project.samples.map(s=>({id:sid(s),file:s.file,name:s.name,type:s.type,runFolder:s.runFolder,method:s.method,panelKey:s.panelKey||'',panel:s.panel||'',
   sizeStd:s.sizeStd,status:s.status,sampleId:s.sampleId||'',comment:s.comment??null,profileId:s.profileId||'',specimenCategory:s.specimenCategory||'',customControl:s.customControl||'',ud:s.ud||{},ladderQuality:s.ladderQuality??null,ladderUsed:!!s.ladderUsed,alasSkipped:!!s.alasSkipped,referenceBins:!!s.referenceBins,sqOverride:s.sqOverride||null,edits:s.edits||[]})),
  snapshots:{methods:Methods.list().filter(r=>used.has(r.method.name)).map(r=>r.method),tableSetting:TableSettings.active(),
   kits:KitStore.load().kits.filter(k=>kitsUsed.has(k.name)),example:state.kit&&state.project.samples.some(s=>s.panelKey===EXAMPLE_KEY)?
    {kit:state.kit,lq:state.ladderQuality,mq:state.markerQuality}:null}}}
async function writeProject(name,{create=false}={}){
 if(state.busy)return false;if(!Sec.need('UPDATE_PROJECT'))return false;
 const isNew=create||!state.project.savedAs;
 const ch=await Sec.change({map:'Project',attribute:isNew?'created':'modified',instance:name,old:isNew?'':(state.project.savedAs||''),new:name,event:isNew?'Add Project':'Modify Project',
  esig:{group:'GM_PROJECT',action:isNew?'CREATE_PROJECT':'MODIFY_PROJECT'}});if(!ch)return false;
 state.busy=true;render();
 try{progress(0,1,T('saving'));
  await jsonPost('/api/idx-project',{action:'save_meta',name,project:projectMeta(),create,overwrite:!create});
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
 dialog(T('saveProjectAs').replace(/\.+$/,''),b=>b.append(el('div',{class:'field'},el('label',{text:T('projectName'),for:'spName'}),el('input',{type:'text',id:'spName',value:state.project.name||''})),
   el('div',{class:'field'},el('label',{text:T('projectComment')}),el('input',{type:'text',id:'spComment',value:state.project.comment||''}))),
  [[T('ok'),async b=>{const n=b.querySelector('#spName').value.trim();if(!n)return false;state.project.comment=b.querySelector('#spComment').value;
    const existing=(await jsonPost('/api/idx-project',{action:'list'})).projects.some(p=>p.name===n);
    if(existing&&n!==state.project.savedAs&&!(await Methods.ask(T('projectOverwrite').replace('{0}',n))))return false;
    for(const s of state.project.samples)s.savedRev=undefined;
    await writeProject(n,{create:n!==state.project.savedAs&&!existing})},true],[T('cancel'),()=>true]])}
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
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('projectName'),T('created'),T('colLastSaved'),T('samples'),T('projectComment')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');t.append(tb);
  const fill=()=>{tb.replaceChildren();const f=q.value.toLowerCase();
   for(const r of rows.projects.filter(r=>r.name.toLowerCase().includes(f))){const tr=el('tr',{class:r.name===selected?'sel':''},el('td',{text:r.name}),el('td',{text:(r.created||'').replace('T',' ')}),
     el('td',{text:(r.modified||'').replace('T',' ')}),el('td',{class:'num',text:String(r.samples)}),el('td',{text:r.comment||''}));
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
  if(snap.kits&&snap.kits.length){const db=KitStore.load();let added=false;for(const k of snap.kits)if(!db.kits.some(x=>x.name===k.name)){db.kits.push(k);added=true;notes.push(T('panelMgrTitle')+': '+k.name)}
   if(added){KitStore.save(db);dropCaches()}}
  if(snap.tableSetting&&!TableSettings.get(snap.tableSetting.name)){try{TableSettings.put(snap.tableSetting);notes.push(T('mgrTables')+': '+snap.tableSetting.name)}catch(e){}}
  if(snap.example){state.kit=snap.example.kit;state.ladderQuality=snap.example.lq;state.markerQuality=snap.example.mq;
   kitCache.set(EXAMPLE_KEY,{kit:snap.example.kit,lq:snap.example.lq,mq:snap.example.mq,policy:'reject'})}
  const samples=[];let i=0;state.nextId=1;
  for(const m of meta.samples){const d=await jsonPost('/api/idx-project',{action:'open_sample',name,id:m.id});const data=d.data||{};
   const id=state.nextId++;samples.push({id,file:m.file,name:m.name,type:m.type,runFolder:m.runFolder,method:m.method,panelKey:m.panelKey,panel:m.panel,sizeStd:m.sizeStd,
    status:data.analysis?m.status:'unanalyzed',sampleId:m.sampleId,comment:m.comment,profileId:m.profileId,specimenCategory:m.specimenCategory||'',customControl:m.customControl||'',ud:m.ud||{},ladderQuality:m.ladderQuality,ladderUsed:!!m.ladderUsed,alasSkipped:!!m.alasSkipped,referenceBins:!!m.referenceBins,sqOverride:m.sqOverride||undefined,edits:m.edits||[],
    blob:d.fsa_b64?b64ToBlob(d.fsa_b64,m.file):null,analysis:data.analysis||null,compact:data.compact||null,report:data.report||null,rev:0,savedRev:0});
   progress(++i,meta.samples.length,`${T('opening')}: ${m.file}`)}
  Object.assign(state.project,{mixture:meta.mixture&&typeof meta.mixture==='object'?meta.mixture:{},reports:Array.isArray(meta.reports)?meta.reports:[],name:meta.name,savedAs:meta.name,comment:meta.comment||'',runFolders:meta.run_folders||[...new Set(samples.map(s=>s.runFolder))],samples,dirty:false});
  state.selected.clear();state.filter=null;state.sort=null;
  state.lastMessage=T('projectOpened')+': '+meta.name;rememberProject(meta.name);if(notes.length)message(T('projectAddedObjects')+'\n'+notes.join('\n'))}
 catch(e){message(e.message)}finally{state.busy=false;render()}}
function rememberProject(name){try{localStorage.setItem('opentrace.lastProject',name)}catch(e){}}
async function closeProject(){if(!(await confirmDiscard()))return;
 Object.assign(state.project,{name:null,savedAs:null,comment:'',runFolders:[],samples:[],reports:[],dirty:false});state.selected.clear();state.lastMessage=null;render()}
// ---------- copy / fill down ----------
function fillDown(){const key=state.lastField;if(!key)return message(T('fillDownHint'));
 const rows=sortedSamples().filter(s=>state.selected.has(s.id));if(rows.length<2)return message(T('fillDownHint'));
 const get=s=>key.startsWith('ud.')?(s.ud||{})[key.slice(3)]||'':(s[key]||'');const v=get(rows[0]);
 for(const s of rows.slice(1))setField(s,key,v);render()}
// ---------- size standards and Size Match Editor ----------
const firstSelected=()=>sortedSamples().find(s=>state.selected.has(s.id));
function editSizeStandard(){const name=firstSelected()?.sizeStd||SizeStd.names()[0];const s=SizeStd.get(name);if(!s)return message(T('arnmStandard'));
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
    Object.assign(state.project,{name:v,savedAs:null,comment:'',runFolders:[],samples:[],reports:[],dirty:true});state.selected.clear();state.filter=null;render()},true],[T('cancel'),()=>true]])}
// File-name heuristics for sample type; the user can change the type in the table.
function sampleTypeFromName(n){
 if(/ladder|allelic|_al_|(^|[_-])lad([_-]|$)|^L\d*[_-]/i.test(n))return 'ladder';
 if(/neg|^NTC|[_-]NTC|^NC\d*[_-]|[_-]NC\d*[_-]|blank/i.test(n))return 'negative';
 // "007" is the AmpFlSTR control DNA only as a whole name part at the start (not a well/plate index like _007_D07).
 if(/pos|^PC\d*[_-]|[_-]PC\d*[_-]|control|9947|2800M|^007([_-]|$)/i.test(n))return 'positive';
 return 'sample'}
function addSamples(){
 if(!Sec.need('UPDATE_PROJECT'))return;
 if(!state.project.name)Object.assign(state.project,{name:T('untitled'),dirty:true});
 let files=[];
 dialog(T('addTitle'),b=>{
  const list=el('div',{class:'filelist'});const input=el('input',{type:'file',multiple:'',accept:'.fsa,.ab1',style:'display:none'});
  const redraw=()=>{list.replaceChildren(...files.map(f=>el('div',{text:f.name})))};
  input.addEventListener('change',()=>{files=[...files,...input.files];input.value='';redraw()});
  const choose=el('button',{class:'btn',text:T('addChoose'),type:'button'});choose.addEventListener('click',()=>input.click());
  const clear=el('button',{class:'btn',text:T('addClear'),type:'button'});clear.addEventListener('click',()=>{files=[];redraw()});
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
  b.append(el('div',{text:T('addFiles')}),list,el('div',{style:'display:flex;gap:6px;margin:6px 0'},choose,clear,input),fs)},
  [[T('add'),body=>{if(!files.length)return false;const run=body.querySelector('#addRun').value.trim()||'Run';const std=body.querySelector('#addStd').value;const pkey=body.querySelector('#addPanel').value;const meth=body.querySelector('#addMethod').value;
    try{localStorage.setItem('opentrace.lastMethod',meth)}catch(e){}
    try{localStorage.setItem('opentrace.lastPanel',pkey)}catch(e){}
    if(!state.project.runFolders.includes(run))state.project.runFolders.push(run);
    for(const f of files){const name=f.name.replace(/\.(fsa|ab1)$/i,'');
     state.project.samples.push({id:state.nextId++,file:f.name,name,type:ProjectOptions.get().add.type!=='auto'?ProjectOptions.get().add.type:sampleTypeFromName(f.name),
      runFolder:run,method:meth,panelKey:pkey,panel:pkey?pkey.split('\u0001')[1]:'',sizeStd:std,status:'unanalyzed',blob:f,analysis:null,report:null})}
    Sec.record(files.map(f=>({map:'Sample',attribute:'created',instance:`${run}/${f.name}`,old:'',new:f.name,event:'Add Sample'})));
    state.project.dirty=true;render()},true],[T('cancel'),()=>true]])}

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
 state.busy=true;state.stop=false;state.lastMessage=null;state.noLadder=null;state.binSetNote=null;render();
 const order=[...targets].sort((a,b)=>(a.type==='ladder'?0:1)-(b.type==='ladder'?0:1)); // ladders first
 let i=0,finished=false;
 try{
  for(const s of order){
   if(state.stop)break;
   progress(i,order.length,`${T('analyzing')}: ${s.file}`);
   if(s.blob){const p=new URLSearchParams({...(await analyzeSettings(s.method,s.sizeStd)),filename:s.file});
    if(s.sqOverride)p.set('sizing_override',JSON.stringify(s.sqOverride));
    const r=await fetch('/api/analyze?'+p,{method:'POST',headers:{'X-Session-Token':TOKEN},body:s.blob});
    const j=await r.json();if(!r.ok)throw Error(`${s.file}: ${j.error}`);s.analysis=j;s.compact=null;s.rev=(s.rev||0)+1}
   s.status='analyzed';i++;progress(i,order.length)}
  finished=await genotype();
  const any=state.project.samples.some(s=>s.panelKey);
  const skipped=state.project.samples.filter(s=>s.alasSkipped).length;
  state.lastMessage=skipped?T('alasSkippedMsg').replace('{0}',skipped):state.noLadder?T('noLadderInRun')+' '+state.noLadder:any?T('analysisDone')+(state.binSetNote?' — '+T('binSetSubstituted')+' '+state.binSetNote:''):T('noKit');
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
 return g}
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
  for(const s of g.group.filter(x=>x.type!=='ladder')){
   if(state.stop)return false;
   const k=kits[s.method];if(k.binSetSubstituted)state.binSetNote=k.binSet;
   $('statusText').textContent=`${T('genotyping')}: ${s.file}`;
   const policy=g.pass?k.policy:'reference_bins';
   let rep=await jsonPost('/api/str-run',{sample:s.compact||compactAnalysis(s.analysis),ladders:g.ladders,kit:k.kit,ladder_quality_definition:k.lq,missing_offset_policy:policy});
   if(k.mq)rep=await jsonPost('/api/marker-quality',{report:rep,definition:k.mq});
   if(s.type==='positive'||s.type==='negative'){const def=controlDefinition(s);if(def)try{rep=await jsonPost('/api/controls',{report:rep,definition:def})}catch(e){rep.quality=rep.quality||{};rep.quality.controls_error=e.message}}
   s.report=rep;s.referenceBins=!g.pass;s.rev=(s.rev||0)+1}}
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
 return {lang:I18N.lang,project:state.project.name||'',printComment:po.printComment?po.comment:'',kit:(samples.map(sampleKit).find(Boolean))||null,samples:samples.map(s=>{const eff=s.report?effective(s):null;const rows=genoRows(s);
  return {id:s.id,file:s.file,name:s.name,type:s.type,analysis:s.analysis,report:s.report?{...s.report,calls:eff.calls}:null,kit:sampleKit(s),pqv:sampleFlags(s),
   markers:rows.map(r=>({marker:r.marker,channel:r.channel,gq:r.gq,gqCat:r.flags.GQ,calls:r.calls,edited:r.flags.ME==='check'})),editable:!!s.report}})}}
function openPanelManager(){if(!Sec.need('READ_PANEL'))return;const w=window.open('/idx/panels.html','opentrace-panels','width=1180,height=800');if(!w)message(T('plotWindowBlocked'));else w.focus()}
function displayPlots(){
 if(!state.selected.size)return message(T('selectSamplesFirst'));
 const w=window.open('/idx/plot.html','opentrace-plot','width=1100,height=760');
 if(!w)message(T('plotWindowBlocked'));else{state.plotWin=w;w.focus()}}

// ---------- export format (ID-X "Format of Exported, Copied, and Report Results") ----------
const STATUS_TEXT={analyzed:'Analyzed',unanalyzed:'Not Analyzed',changed:'Analysis setting changed'};
const BOOL_COLS=new Set(['SE','SQO','ADO','ME']);
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
const blankRow=s=>({sample:s,marker:'',channel:'',calls:[],gq:null,flags:{},peaks:[],reason:''});
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
window.OT.report=reportApi;

// ---------- printing: page setup and print preview ----------
const PAGE_KEY='opentrace.pageSetup.v1';
function pageSetup(){let v={};try{v=JSON.parse(localStorage.getItem(PAGE_KEY)||'{}')||{}}catch(e){}
 return {orientation:v.orientation==='portrait'?'portrait':'landscape',fit:v.fit!==false,header:v.header!==false,paper:['A4','Letter','Legal'].includes(v.paper)?v.paper:'A4'}}
function pageSetupDialog(){const o=pageSetup();
 dialog(T('pageSetup').replace(/\.+$/,''),b=>{b.classList.add('psdlg');
  const paper=el('select',{id:'psPaper'});for(const p of ['A4','Letter','Legal'])paper.append(new Option(p,p));paper.value=o.paper;paper.addEventListener('change',()=>o.paper=paper.value);
  const or=el('select',{id:'psOrient'});or.append(new Option(T('psLandscape'),'landscape'),new Option(T('psPortrait'),'portrait'));or.value=o.orientation;or.addEventListener('change',()=>o.orientation=or.value);
  const fit=el('input',{type:'checkbox',id:'psFit'});fit.checked=o.fit;fit.addEventListener('change',()=>o.fit=fit.checked);
  const hd=el('input',{type:'checkbox',id:'psHeader'});hd.checked=o.header;hd.addEventListener('change',()=>o.header=hd.checked);
  b.append(el('div',{class:'field'},el('label',{text:T('psPaper')}),paper),el('div',{class:'field'},el('label',{text:T('psOrientation')}),or),
   el('label',{class:'chk'},fit,document.createTextNode(' '+T('psFit'))),el('label',{class:'chk'},hd,document.createTextNode(' '+T('psHeaderText'))))},
  [[T('ok'),()=>{try{localStorage.setItem(PAGE_KEY,JSON.stringify(o))}catch(e){}},true],[T('cancel'),()=>true]])}
function printPayload(){
 const tab=state.tab;let title=T(tab==='summary'?'analysisSummary':tab),table=null,html=null;
 if(tab==='summary')html=$('summary').innerHTML;else table=tableRows(tab==='genotypes'?'genotypes':'samples',false);
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
function selectAll(){state.selected=new Set(visibleSamples().map(s=>s.id));render()}
function setLang(l){I18N.set(l);buildChrome();render()}

// ---------- chrome ----------
function buildChrome(){
 document.documentElement.lang=I18N.lang;
 const lang=I18N.lang;
 buildMenubar($('menubar'),[
  {label:T('file'),items:[{label:T('newProject'),sc:'Ctrl+N',run:newProject},{label:T('openProject'),sc:'Ctrl+O',run:openProject},{label:T('closeProject'),run:closeProject},
   {label:T('saveProject'),sc:'Ctrl+S',run:saveProject},{label:T('saveProjectAs'),run:saveProjectAs},'-',
   {label:T('addSamples'),sc:'Ctrl+K',run:addSamples},{label:T('associateSamples'),disabled:true},'-',
   {label:T('exportTable'),sc:'Ctrl+E',run:exportTable},{label:T('exportCombined'),run:exportCombined},{label:T('exportCodis'),run:()=>Profiles.exportCodis()},'-',
   {label:T('projectOptions'),run:projectOptions},'-',{label:T('pageSetup'),run:pageSetupDialog},{label:T('printPreview'),run:printPreview},{label:T('print'),sc:'Ctrl+P',run:printNow},'-',
   {label:T('logOut'),run:()=>Sec.logout(),disabled:!Sec.state.require_login},{label:T('scChangePassword')+'...',run:()=>Sec.changePassword(),disabled:!Sec.user?.has_password},{label:T('exit'),run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('undo'),sc:'Ctrl+Z',disabled:true},{label:T('redo'),sc:'Ctrl+Y',disabled:true},'-',{label:T('copy'),sc:'Ctrl+C',run:copyRows},{label:T('paste'),sc:'Ctrl+V',disabled:true},
   {label:T('clear'),disabled:true},{label:T('selectAll'),sc:'Ctrl+A',run:selectAll},{label:T('fillDown'),sc:'Ctrl+D',run:fillDown},'-',{label:T('deleteFromProject'),run:deleteFromProject},'-',
   {label:T('find'),sc:'Ctrl+F',run:findDialog},{label:T('sort'),run:sortDialog}]},
  {label:T('analysis'),items:[{label:T('analyze'),sc:'Ctrl+R',run:()=>analyze('pending')},{label:T('analyzeSelected'),run:()=>analyze('selected')},{label:T('analyzeAll'),run:()=>analyze('all')},'-',
   {label:T('lowQualityTop'),sc:'Ctrl+B',checked:state.lowQualityTop,run:()=>{state.lowQualityTop=!state.lowQualityTop;buildChrome();render()}},{label:T('nonConcordantTop'),disabled:true},'-',
   {label:T('displayPlots'),sc:'Ctrl+L',run:displayPlots},'-',{label:T('analysisMethodEditor'),run:()=>editMethod(state.project.samples.find(s=>state.selected.has(s.id))?.method)},{label:T('sizeStandardEditor'),run:editSizeStandard},{label:T('sizeMatchEditor'),run:openSizeMatch},{label:T('binOffsets'),run:notYet}]},
  {label:T('view'),items:[{label:T('samples'),checked:state.tab==='samples',run:()=>{buildAfter(()=>showTab('samples'))}},{label:T('analysisSummary'),checked:state.tab==='summary',run:()=>buildAfter(()=>showTab('summary'))},
   {label:T('genotypes'),checked:state.tab==='genotypes',run:()=>buildAfter(()=>showTab('genotypes'))},'-',{label:T('sampleInfo'),run:notYet},{label:T('rawData'),run:notYet},{label:T('eptData'),run:notYet},'-',
   {label:T('showNavigator'),checked:state.showNavigator,run:()=>{state.showNavigator=!state.showNavigator;buildChrome();render()}},{label:T('fitToContent'),run:notYet},'-',
   {label:T('language'),sub:[{label:'Русский',checked:lang==='ru',run:()=>setLang('ru')},{label:'English',checked:lang==='en',run:()=>setLang('en')}]}]},
  {label:T('tools'),items:[{label:T('manager'),sc:'Ctrl+M',run:()=>Methods.openManager()},{label:T('panelManager'),sc:'Ctrl+J',run:openPanelManager},{label:T('reportManager'),sc:'Ctrl+Q',run:openReportManager},{label:T('labelEditViewer'),run:labelEditViewer},
   {label:T('tableSettingEditor'),sc:'Ctrl+T',run:editTableSetting},{label:T('codisManager'),run:()=>Profiles.codisManager()},'-',{label:T('addProfile'),sub:[{label:T('pfLab'),run:()=>addProfile('Lab Reference')},{label:T('pfCustom'),run:()=>addProfile('Custom Control')}]},{label:T('profileManager'),run:()=>Profiles.profileManager()},{label:T('profileComparison'),run:()=>Profiles.profileComparison()},'-',
   {label:T('mixture'),sc:'Ctrl+U',run:()=>Mixture.open()},{label:T('mixtureManager'),sc:'Ctrl+Y',run:()=>Mixture.manager()}]},
  {label:T('admin'),items:[{label:T('securityManager'),run:()=>Sec.securityManager(),disabled:!Sec.can('AdminToolAcc/General/May run application')},
   {label:T('auditManager'),sub:[{label:T('auditSetting'),run:()=>Sec.auditSettings(),disabled:!Sec.can('AuditGUI/Map Configuration Tool/May run application')},
    {label:T('auditHistory'),run:()=>Sec.auditHistory(),disabled:!Sec.can('AuditGUI/Audit History Viewer/May run application')},'-',
    {label:T('scShowCount'),run:()=>Sec.auditCount()},{label:T('scBackupMenu'),run:()=>Sec.auditBackup(),disabled:!Sec.can('AuditGUI/Audit Management/Backup Audit Records')},
    {label:T('scRestoreMenu'),run:()=>Sec.auditRestore(),disabled:!Sec.can('AuditGUI/Audit Management/Restore Audit Records')},{label:T('scDeleteMenu'),run:()=>Sec.auditDelete(),disabled:!Sec.can('AuditGUI/Audit Management/Delete Audit Records')}]},
   {label:T('esig'),run:()=>Sec.esigAdmin(),disabled:!Sec.can('ESig/ESig Administration Tool/May run application')},'-',{label:T('scRemoteShutdown'),disabled:true},{label:T('databaseManager'),disabled:true}]},
  {label:T('help'),items:[{label:T('helpTopics'),sc:'F1',run:notYet},{label:T('loadExample'),run:loadExample},'-',{label:T('about'),run:()=>message(T('aboutText'),T('about'))}]},
 ]);
 const tb=$('toolbar');tb.replaceChildren(
  tbutton('newProject',T('newProject'),newProject),tbutton('openProject',T('openProject'),openProject,{id:'tbOpen'}),tbutton('saveProject',T('saveProject'),saveProject,{id:'tbSave'}),tsep(),
  tbutton('addSamples',T('addSamples'),addSamples),tbutton('exportTable',T('exportTable'),exportTable,{id:'tbExport'}),tsep(),
  tbutton('displayPlots',T('displayPlots'),displayPlots,{id:'tbPlots'}),tbutton('reportManager',T('reportManager'),openReportManager,{id:'tbReport'}),tbutton('labelEditViewer',T('labelEditViewer'),labelEditViewer,{id:'tbLabelEdits'}),
  tbutton('sizeMatchEditor',T('sizeMatchEditor'),openSizeMatch,{id:'tbSizeMatch'}),tsep(),tbutton('analysisMethodEditor',T('analysisMethodEditor'),()=>editMethod(state.project.samples.find(s=>state.selected.has(s.id))?.method),{id:'tbMethodEditor'}),tbutton('panelManager',T('panelManager'),openPanelManager,{id:'tbPanelManager'}),
  tbutton('manager',T('manager'),()=>Methods.openManager(),{id:'tbManager'}),tsep(),tbutton('analyze',T('analyze'),()=>analyze('pending'),{id:'tbAnalyze'}),tbutton('lowQualityTop',T('lowQualityTop'),()=>{state.lowQualityTop=!state.lowQualityTop;render()}),
  tsep(),el('span',{class:'tlabel',text:T('tableSetting')}));
 tb.append(tableSettingCombo(),tbutton('tableSettingEditor',T('tableSettingEditor'),editTableSetting,{id:'tbTableSetting'}),tsep(),tbutton('find',T('find'),findDialog,{id:'tbFind'}),tbutton('print',T('print'),printNow,{id:'tbPrint'}),
  tbutton('printPreview',T('printPreview'),printPreview,{id:'tbPrintPreview'}),tbutton('help',T('helpTopics'),notYet));
 $('stopBtn').textContent=T('stop');
 renderTabs()}
function projectOptions(){ProjectOptions.open({methodSelect,panelSelect,sizeStdNames:()=>Object.keys(SIZE_STANDARDS),types:Object.entries(TYPE_KEYS).map(([k,v])=>[k,T(v)])})}
function buildAfter(fn){fn();buildChrome();render()}

$('stopBtn').addEventListener('click',()=>{state.stop=true});
document.addEventListener('keydown',e=>{
 if(e.target.closest('input,select,textarea'))return;
 const k=e.key.toLowerCase();
 if(e.key==='F1'){e.preventDefault();notYet();return}
 if(!e.ctrlKey&&!e.metaKey)return;
 const map={u:()=>Mixture.open(),y:()=>Mixture.manager(),o:openProject,s:saveProject,d:fillDown,c:copyRows,t:editTableSetting,f:findDialog,m:()=>Methods.openManager(),j:openPanelManager,n:newProject,k:addSamples,e:exportTable,l:displayPlots,q:openReportManager,p:printNow,r:()=>analyze('pending'),a:selectAll,b:()=>{state.lowQualityTop=!state.lowQualityTop;render()}};
 if(map[k]){e.preventDefault();map[k]()}});
window.addEventListener('beforeunload',e=>{if(state.project.dirty&&state.project.samples.length){e.preventDefault();e.returnValue=''}});

function qcSamples(){return state.project.samples.map(s=>{const eff=s.report?effective(s):null;const markers={},dye={};
 if(eff)for(const c of eff.calls){if(!c.label)continue;(markers[c.marker]=markers[c.marker]||[]).push(String(c.label));dye[c.marker]=dyeLetter(c.channel)}
 const q=samplePqv(s).SQ;const ol=Object.values(markers).some(a=>a.some(x=>/^OL$/i.test(x)));const ud=s.ud||{};const info=s.analysis?.run_info||{};
 return {id:s.file,file:s.file,name:s.name,type:s.type,analyzed:!!s.report,profileId:(s.profileId||'').trim(),markers,dye,category:s.specimenCategory||'No Export',
  eligible:!!s.report&&s.type!=='ladder'&&['pass','check','override'].includes(q)&&!ol&&Object.keys(markers).length>0,
  codis:{sample_name:s.name,specimen_id:ud.UD1||s.name,category:s.specimenCategory||'No Export',panel:s.panel||'',batch_id:ud.UD2||info.plate||'',case_id:ud.UD3||'',
   comment:ud.UD4||'',partial:ud.UD5||'',source_id:ud.UD6||'',ncic:ud.UD7||'',vicap:ud.UD8||'',loci:Object.entries(markers).map(([marker,alleles])=>({marker,alleles}))}}})}
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
Methods.init(jsonPost);
buildChrome();render();
window.addEventListener('opentrace-security',()=>{buildChrome();render()});
Sec.init().then(ok=>{if(ok===false)return;buildChrome();render();
 Methods.ensureDefault().then(()=>{render();
  let last=null;try{last=localStorage.getItem('opentrace.lastProject')}catch(e){}
  if(ProjectOptions.get().startup==='last'&&last&&Sec.can('READ_PROJECT'))return loadProject(last)}).catch(e=>message(e.message))});
})();
