// Report Settings: columns from the Samples and Genotypes tables plus custom columns;
// Report Settings Editor and GeneMapper ReportSettingContainer XML (process-profile).
(function(){
const KEY='opentrace.reports.v1';
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
// Shipped "Allele Table" (Database Defaults/Report Settings/Allele Table.xml).
const DEFAULTS=[{name:'Allele Table',description:'',numAlleles:2,custom:[],columns:[
 {cat:'Genotype',id:'sampleFile',show:true},{cat:'Sample',id:'sampleName',show:true},{cat:'Genotype',id:'marker',show:true},
 {cat:'Genotype',id:'allele',index:0,show:true},{cat:'Genotype',id:'allele',index:1,show:true}]}];
const catalog=cat=>cat==='Sample'?TableSettings.CATALOG.samples:TableSettings.CATALOG.genotypes;
const isAllele=(cat,id)=>cat==='Genotype'&&catalog('Genotype').find(c=>c.id===id)?.kind==='allele';
function colHead(c){if(c.cat==='Custom')return c.id;const d=catalog(c.cat).find(x=>x.id===c.id);if(!d)return c.id;return isAllele(c.cat,c.id)?`${d.head} ${(c.index||0)+1}`:d.head}
const sameCol=(a,b)=>a.cat===b.cat&&a.id===b.id&&(a.index||0)===(b.index||0);

function load(){try{const v=JSON.parse(localStorage.getItem(KEY)||'null');if(v&&Array.isArray(v.settings)&&v.settings.length)return v}catch(e){}
 const now=new Date().toISOString();return {schema:1,settings:DEFAULTS.map(s=>({setting:clone(s),saved:now}))}}
function store(db){try{localStorage.setItem(KEY,JSON.stringify(db));return true}catch(e){return false}}
function list(){return load().settings}
function names(){return list().map(r=>r.setting.name)}
function get(name){return list().find(r=>r.setting.name===name)?.setting||null}
function validate(s){
 if(!s||typeof s.name!=='string'||!s.name.trim()||s.name.length>240)throw Error(T('enterName'));
 const n=Number(s.numAlleles);if(!Number.isInteger(n)||n<1||n>20)throw Error(T('numAllelesRange'));s.numAlleles=n;
 s.description=String(s.description||'');
 s.custom=(Array.isArray(s.custom)?s.custom:[]).filter(c=>c&&typeof c.name==='string'&&c.name.trim()).map(c=>({name:c.name.trim().slice(0,120),description:String(c.description||'').slice(0,500)}));
 const customNames=new Set(s.custom.map(c=>c.name));
 if(!Array.isArray(s.columns))throw Error(T('rsNoColumns'));
 const out=[];
 for(const c of s.columns){if(!c||!['Sample','Genotype','Custom'].includes(c.cat))continue;
  if(c.cat==='Custom'){if(!customNames.has(c.id))continue}
  else if(!catalog(c.cat).some(x=>x.id===c.id))continue;
  const col={cat:c.cat,id:c.id,show:c.show!==false};if(isAllele(c.cat,c.id)){col.index=Math.max(0,Math.min(n-1,parseInt(c.index||0,10)||0))}
  if(!out.some(x=>sameCol(x,col)))out.push(col)}
 if(!out.length)throw Error(T('rsNoColumns'));
 s.columns=out;return s}
function put(s,previous){validate(s);const db=load();const i=db.settings.findIndex(r=>r.setting.name===(previous??s.name));
 const row={setting:clone(s),saved:new Date().toISOString()};if(i>=0)db.settings[i]=row;else db.settings.push(row);
 if(!store(db))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-reports'))}
function remove(name){const db=load();db.settings=db.settings.filter(r=>r.setting.name!==name);store(db);window.dispatchEvent(new Event('opentrace-reports'))}

// ---------- ReportSettingContainer XML ----------
const b64dec=s=>new TextDecoder().decode(Uint8Array.from(atob(s.replace(/\s+/g,'')),c=>c.charCodeAt(0)));
const b64enc=s=>{const b=new TextEncoder().encode(s);let x='';for(const c of b)x+=String.fromCharCode(c);return btoa(x)};
const esc=x=>String(x).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
function parseSafe(text){if(text.length>2097152||/<!DOCTYPE|<!ENTITY/i.test(text))throw Error(T('xmlInvalid'));
 const doc=new DOMParser().parseFromString(text,'application/xml');if(doc.getElementsByTagName('parsererror').length)throw Error(T('xmlInvalid'));return doc}
function byHead(cat,head){
 if(cat==='Sample'||cat==='Genotype'){
  for(const c of catalog(cat)){if(c.head===head)return {cat,id:c.id};
   if(c.kind==='allele'){const m=new RegExp('^'+c.head.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')+' (\\d+)$').exec(head);if(m)return {cat,id:c.id,index:parseInt(m[1],10)-1}}}}
 return null}
function fromXml(text){
 const doc=parseSafe(text);const node=doc.getElementsByTagName('xmlReportSetting')[0];if(!node)throw Error(T('xmlInvalid'));
 const one=(n,t)=>t.getElementsByTagName(n)[0]?.textContent.trim();
 const content=one('content',node);if(!content)throw Error(T('xmlInvalid'));
 const prof=parseSafe(b64dec(content));
 const s={name:one('reportSettingName',node)||one('name',prof)||'Imported',description:one('comments',node)||'',numAlleles:parseInt(one('user-parameter',prof)||'2',10)||2,custom:[],columns:[]};
 const skipped=[];
 for(const dc of prof.getElementsByTagName('data-column')){const name=one('name',dc)||'';const cat=one('category',dc)||'';const show=one('display',dc)!=='false';
  if(/^custom$/i.test(cat)){s.custom.push({name,description:one('description',dc)||''});s.columns.push({cat:'Custom',id:name,show});continue}
  const c=byHead(cat==='Sample'?'Sample':'Genotype',name)||byHead(cat==='Sample'?'Genotype':'Sample',name);
  if(c){c.show=show;s.columns.push(c)}else skipped.push(name)}
 s.numAlleles=Math.max(1,Math.min(20,Math.max(s.numAlleles,...s.columns.filter(c=>c.index!=null).map(c=>c.index+1))));
 const out=validate(s);out._skipped=skipped;return out}
function toXml(s){
 const cols=s.columns.map(c=>{const head=colHead(c);const desc=c.cat==='Custom'?(s.custom.find(x=>x.name===c.id)?.description||head):head;
  return `<data-column><name>${esc(head)}</name><description>${esc(desc)}</description><alias>${esc(head)}</alias><category>${c.cat}</category><display>${c.show?'true':'false'}</display><dataType>String</dataType><active>true</active></data-column>`}).join('');
 const prof=`<?xml version="1.0" encoding="UTF-8"?>\n<process-profile xmlns="http://www.appliedbiosystems.com/GeneMapper" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="process-profile"><name>${esc(s.name)}</name><process-header>${cols}</process-header><step/><user-parameter>${s.numAlleles}</user-parameter></process-profile>`;
 const now=Date.now();
 return '<?xml version="1.0" encoding="UTF-8"?>\n<ReportSettingContainer xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="ReportSettingContainer"><appVersion>1.0</appVersion><fileVersion>1.0</fileVersion>'+
  `<xmlReportSetting><reportSettingName>${esc(s.name)}</reportSettingName><userName>OpenTrace</userName><createdDate>${now}</createdDate><modifiedDate>${now}</modifiedDate><content>${b64enc(prof)}</content><comments>${esc(s.description||'')}</comments></xmlReportSetting></ReportSettingContainer>\n`}

// ---------- Report Settings Editor ----------
function openEditor(source,{title,onSave,exists}={}){
 const s=clone(source);const previous=source.name;let tab='Sample',availSel=null,selIdx=null,customSel=null;
 const d=dialog(title||T('reportSettingsEditor'),()=>{},[[T('ok'),()=>{try{s.name=s.name.trim();
    if(s.name!==previous&&(exists?exists(s.name):get(s.name))){message(fmt('methodNameExists',s.name));return false}
    validate(s);onSave(s,previous);return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]]);
 const available=()=>{if(tab==='Custom')return s.custom.map(c=>({cat:'Custom',id:c.name}));
  const out=[];for(const c of catalog(tab)){if(c.kind==='allele'&&tab==='Genotype')for(let i=0;i<s.numAlleles;i++)out.push({cat:tab,id:c.id,index:i});else out.push({cat:tab,id:c.id})}
  return out.filter(a=>!s.columns.some(x=>sameCol(x,a)))};
 const addCol=a=>{s.columns.push({...a,show:true});availSel=null;draw()};
 const customDialog=(cur)=>{const c=cur?{...cur}:{name:'',description:''};
  dialog(T(cur?'rsEditCustom':'rsCreateCustom'),bb=>bb.append(el('div',{class:'field'},el('label',{text:T('amName')}),el('input',{type:'text',id:'rcName',value:c.name})),
    el('div',{class:'field'},el('label',{text:T('amDescription')}),el('input',{type:'text',id:'rcDesc',value:c.description}))),
   [[T('ok'),bb=>{const n=bb.querySelector('#rcName').value.trim();if(!n)return false;
     if(s.custom.some(x=>x.name===n&&x!==cur)||catalog('Sample').some(x=>x.head===n)||catalog('Genotype').some(x=>x.head===n)){message(fmt('methodNameExists',n));return false}
     if(cur){for(const col of s.columns)if(col.cat==='Custom'&&col.id===cur.name)col.id=n;cur.name=n;cur.description=bb.querySelector('#rcDesc').value}
     else s.custom.push({name:n,description:bb.querySelector('#rcDesc').value});draw()},true],[T('cancel'),()=>true]])};
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('rsdlg');
  const name=el('input',{type:'text',id:'rsName',value:s.name});name.addEventListener('input',()=>{s.name=name.value});
  const desc=el('input',{type:'text',id:'rsDesc',value:s.description||''});desc.addEventListener('input',()=>{s.description=desc.value});
  const na=el('input',{type:'number',id:'rsAlleles',min:'1',max:'20',value:String(s.numAlleles),class:'num'});
  na.addEventListener('change',()=>{const v=Math.max(1,Math.min(20,parseInt(na.value,10)||1));s.numAlleles=v;s.columns=s.columns.filter(c=>c.index==null||c.index<v);draw()});
  const tabs=el('div',{class:'tabs dtabs'});
  for(const [id,key] of [['Sample','samples'],['Genotype','genotypes'],['Custom','rsCustom']]){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key),'data-tab':id});t.addEventListener('mousedown',()=>{tab=id;availSel=null;draw()});tabs.append(t)}
  const av=el('select',{id:'rsAvailable',size:'14',class:'rslist'});for(const [i,a] of available().entries()){const o=new Option(colHead(a),String(i));if(availSel===i)o.selected=true;av.append(o)}
  av.addEventListener('change',()=>{availSel=parseInt(av.value,10)});av.addEventListener('dblclick',()=>{const a=available()[parseInt(av.value,10)];if(a)addCol(a)});
  const btn=(text,fn,id)=>{const x=el('button',{class:'btn',type:'button',text,...(id?{id}:{})});x.addEventListener('click',fn);return x};
  const left=el('div',{class:'rscol'},av);
  if(tab==='Custom')left.append(el('div',{class:'btnrow'},btn(T('rsCreate'),()=>customDialog(null),'rsCreateBtn'),
   btn(T('rsEdit'),()=>{const c=s.custom.find(x=>x.name===available()[availSel]?.id);if(c)customDialog(c)}),
   btn(T('mgrDelete'),()=>{const id=available()[availSel]?.id;if(!id)return;s.custom=s.custom.filter(x=>x.name!==id);availSel=null;draw()})));
  const mid=el('div',{class:'rsmid'},btn('»',()=>{const a=available()[availSel];if(a)addCol(a)},'rsAdd'),btn('«',()=>{if(selIdx==null)return;s.columns.splice(selIdx,1);selIdx=null;draw()},'rsRemove'));
  const st=el('table',{class:'g rssel'});st.append(el('thead',{},el('tr',{},el('th',{text:T('rsShow')}),el('th',{text:T('colName')}),el('th',{text:T('rsCategory')}))));
  const tb=el('tbody');
  s.columns.forEach((c,i)=>{const cb=el('input',{type:'checkbox'});cb.checked=c.show;cb.addEventListener('change',()=>{c.show=cb.checked;drawPreview()});cb.addEventListener('mousedown',e=>e.stopPropagation());
   const tr=el('tr',{class:selIdx===i?'sel':''},el('td',{class:'ic'},cb),el('td',{text:colHead(c)}),el('td',{text:T('rsCat'+c.cat)}));
   tr.addEventListener('mousedown',()=>{selIdx=i;tb.querySelectorAll('tr').forEach((x,j)=>x.classList.toggle('sel',j===i))});
   tr.addEventListener('dblclick',()=>{s.columns.splice(i,1);selIdx=null;draw()});tb.append(tr)});
  st.append(tb);
  const move=dlt=>{if(selIdx==null)return;const j=selIdx+dlt;if(j<0||j>=s.columns.length)return;const [c]=s.columns.splice(selIdx,1);s.columns.splice(j,0,c);selIdx=j;draw()};
  const right=el('div',{class:'rscol'},el('div',{class:'grid rsgrid'},st),el('div',{class:'btnrow'},btn('▲',()=>move(-1),'rsUp'),btn('▼',()=>move(1),'rsDown')));
  const preview=el('div',{class:'grid rsprev',id:'rsPreview'});
  const drawPreview=()=>{preview.replaceChildren();const t=el('table',{class:'g'});const tr=el('tr');
   for(const c of s.columns.filter(x=>x.show))tr.append(el('th',{text:colHead(c)}));t.append(el('thead',{},tr));preview.append(t)};
  drawPreview();
  b.append(el('div',{class:'field'},el('label',{text:T('amName')}),name),el('div',{class:'field'},el('label',{text:T('amDescription')}),desc),
   el('div',{class:'field'},el('label',{text:T('numAlleles')}),na),
   el('div',{class:'rsbody'},el('div',{},tabs,el('div',{class:'tabpage dpage'},el('div',{class:'cap',text:T('rsAvailable')}),left)),mid,
    el('div',{},el('div',{class:'cap',text:T('rsSelected')}),right)),el('div',{class:'cap',text:T('rsPreview')}),preview)};
 draw();return d}

if(window.Methods)Methods.registerKind(()=>({id:'reports',label:T('mgrReports'),
 rows:()=>list().map(r=>({name:r.setting.name,saved:r.saved,a:String(r.setting.columns.length),b:r.setting.description||''})),
 heads:[T('rsColumns'),T('amDescription').replace(':','')],get,put,remove,count:()=>list().length,
 create:async()=>{let n=1;while(get(`${T('rsUntitled')} ${n}`))n++;return {name:`${T('rsUntitled')} ${n}`,description:'',numAlleles:2,custom:[],columns:clone(DEFAULTS[0].columns)}},
 edit:(obj,opts)=>openEditor(obj,opts),newTitle:T('reportSettingsEditor'),
 importText:async text=>{const obj=/^\s*[{]/.test(text)?validate(JSON.parse(text)):fromXml(text);const sk=obj._skipped||[];delete obj._skipped;
  return {obj,note:sk.length?T('rsSkipped')+' '+sk.join(', '):''}},
 exportText:async obj=>toXml(obj),imported:T('rsImported'),confirmDelete:'confirmDeleteReport'}));

window.ReportSettings={KEY,DEFAULTS,list,names,get,put,remove,validate,fromXml,toXml,openEditor,colHead,isAllele};
})();
