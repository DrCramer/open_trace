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
 s.columns=out;s.steps=cleanSteps(s.steps);return s}
function put(s,previous){validate(s);const db=load();const i=db.settings.findIndex(r=>r.setting.name===(previous??s.name));
 const row={setting:clone(s),saved:new Date().toISOString()};if(i>=0)db.settings[i]=row;else db.settings.push(row);
 if(!store(db))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-reports'))}
function remove(name){const db=load();db.settings=db.settings.filter(r=>r.setting.name!==name);store(db);window.dispatchEvent(new Event('opentrace-reports'))}

// ---------- Calculation / Analysis steps (ID-X ReportMgr f.java + b.java semantics) ----------
const FUNCS=['sum','difference','average','ratio','product','abs','differencesquare','sqrt'];
const BINARY=new Set(['difference','ratio','abs','differencesquare']);
const LEFTS=['AT LEAST','AT MOST','EXACTLY'];
const OPS=['GREATER THAN','GREATER THAN OR EQUAL TO','LESS THAN','LESS THAN OR EQUAL TO','EQUAL TO','NOT','LIKE','NOT EMPTY','EMPTY'];
const CONST='<user>';
const int=(v,lo,hi,d)=>{const n=parseInt(v,10);return Number.isFinite(n)?Math.max(lo,Math.min(hi,n)):d};
function cleanSteps(a){const out=[];for(const x of Array.isArray(a)?a:[]){if(!x)continue;
 const base={kind:x.kind==='analysis'?'analysis':'calc',name:String(x.name||'').trim().slice(0,120),description:String(x.description||'').slice(0,500),active:x.active!==false,
  columns:(Array.isArray(x.columns)?x.columns:[]).map(String).filter(Boolean).slice(0,50),groupRow:int(x.groupRow,1,1000,1),targetColumn:String(x.targetColumn||'').trim().slice(0,120),targetRow:int(x.targetRow,1,1000,1)};
 if(!base.name)continue;
 if(base.kind==='calc'){base.fn=FUNCS.includes(x.fn)?x.fn:'sum';base.referenceIndex=int(x.referenceIndex,0,1000,0);if(!base.targetColumn)continue}
 else{base.specificRow=int(x.specificRow,0,1000,0);base.any=!!x.any;base.append=!!x.append;base.text=String(x.text||'').slice(0,200);
  base.conditions=(Array.isArray(x.conditions)?x.conditions:[]).map(c=>({left:LEFTS.includes(c&&c.left)?c.left:'AT LEAST',ncols:int(c&&c.ncols,1,1000,1),operator:OPS.includes(c&&c.operator)?c.operator:'EQUAL TO',right:String(c&&c.right!=null?c.right:'').slice(0,100)}));
  if(!base.conditions.length||(!base.append&&!base.targetColumn))continue}
 out.push(base)}return [...out.filter(x=>x.kind==='calc'),...out.filter(x=>x.kind==='analysis')]}
const cnum=v=>{if(v==null)return 0;if(typeof v==='number')return v;const t=String(v).trim();if(t==='')return 0;const n=Number(t);return Number.isFinite(n)?n:-Infinity};
const cfmt=x=>x===-Infinity||x===Infinity?'FAILED':Number.isNaN(x)?'NaN':Math.round(x*1e4)/1e4;
function calcFn(fn,a){if(a.some(x=>x===-Infinity))return -Infinity;const sum=a.reduce((x,y)=>x+y,0),[p,q]=[a[0]??0,a[1]??0];
 switch(fn){case 'sum':return sum;case 'average':return a.length?sum/a.length:0;case 'difference':return p-q;case 'ratio':return p/q;
  case 'product':return a.reduce((x,y)=>x*y,1);case 'abs':return Math.abs(p-q);case 'differencesquare':return (p-q)*(p-q);case 'sqrt':return Math.sqrt(sum)}return 0}
const jcmp=(a,b)=>{a=a.toUpperCase().toLowerCase();b=b.toUpperCase().toLowerCase();return a<b?-1:a>b?1:0};
function countMatch(items,op,right,numeric){let n=0;if(!items.length)return 0;
 for(const it of items){let ok=false;
  if(numeric){const a=it,b=Number(right);
   ok=op==='GREATER THAN'?a>b:op==='GREATER THAN OR EQUAL TO'?a>=b:op==='LESS THAN'?a<b:op==='LESS THAN OR EQUAL TO'?a<=b:op==='EQUAL TO'?a===b:op==='NOT'?a!==b:op==='EMPTY'?true:false}
  else{const a=it,b=right;
   ok=op==='GREATER THAN'?jcmp(a,b)>0:op==='GREATER THAN OR EQUAL TO'?jcmp(a,b)>=0:op==='LESS THAN'?jcmp(a,b)<0:op==='LESS THAN OR EQUAL TO'?jcmp(a,b)<=0:op==='EQUAL TO'?jcmp(a,b)===0:op==='NOT'?jcmp(a,b)!==0:
    op==='NOT EMPTY'?(a==='0'?'':a)!=='':op==='EMPTY'?a==='':op==='LIKE'?a.toUpperCase().trim().startsWith(right.toUpperCase().trim()):false}
  if(ok)n++}return n}
// Condition evaluation, as ID-X ReportMgr.b: numeric values compare as numbers when the right side is numeric, otherwise as text.
function evalConds(vals,conds,any){
 const nums=[],strs=[];for(const s of vals){strs.push(s);const n=Number(s);if(s!==''&&Number.isFinite(n)){nums.push(n)}}
 // numeric strings are in both lists; non-numeric only in the text list (Java adds to the text list in both cases)
 const one=c=>{if(!nums.length&&!strs.length)return c.operator==='EMPTY';
  if(c.ncols<=0)return false;const mx=nums.length?Math.max(...nums):0;let k;const right=String(c.right);
  const pi=right.indexOf('%');
  if(pi!==-1){const pct=Number(right.slice(0,pi));const suf=right.slice(pi).toUpperCase();
   // ID-X applies the percentage only to "% OF THE ROW MAXIMUM"; with any other suffix the factor stays 1 (its MINIMUM branch is unreachable)
   const f=suf==='% OF THE ROW MAXIMUM'?mx*(pct/100):1;k=countMatch(nums,c.operator,String(f),true)}
  else if(right.trim()!==''&&Number.isFinite(Number(right)))k=countMatch(nums,c.operator,right,true);
  else k=countMatch(strs,c.operator,right,false);
  return c.left==='AT LEAST'?k>=c.ncols:c.left==='EXACTLY'?k===c.ncols:c.left==='AT MOST'?k<=c.ncols:false};
 if(!conds||!conds.length)return false;
 return any?conds.some(one):conds.every(one)}
// Runs the steps over a report matrix {heads,cols,rows:[{values}]} holding every column of the setting; returns the visible result.
function applySteps(setting,d){
 const n0=d.cols.length;const heads=d.heads.slice();const cols=d.cols.slice();const grid=d.rows.map(r=>r.values.slice());
 const steps=(setting.steps||[]).filter(x=>x.active!==false);
 const col=(name,create)=>{let i=heads.indexOf(name);if(i<0&&create){heads.push(name);cols.push({cat:'Calculated',id:name,show:true});grid.forEach(r=>r.push(''));i=heads.length-1}return i};
 for(const st of steps){const G=Math.max(1,st.groupRow||1);
  if(st.kind==='calc'){
   const ci=st.columns.map(c=>c.startsWith(CONST)?c:col(c,false));const ti=col(st.targetColumn,true);
   const val=(r,c)=>typeof c==='string'?(Number.isFinite(Number(c.slice(CONST.length)))&&c.length>CONST.length?Number(c.slice(CONST.length)):-Infinity):(c<0?-Infinity:cnum(grid[r][c]));
   const put=(r,v)=>{if(r<grid.length)grid[r][ti]=cfmt(v)};
   for(let g=0;g<grid.length;g+=G){const end=Math.min(g+G,grid.length);const tr=Math.min(st.targetRow-1,end-g-1);
    if(st.referenceIndex>0&&ci.length){const ref=g+st.referenceIndex-1;if(ref>=end)continue;const rv=val(ref,ci[0]);
     for(let r=g;r<end;r++){if(r===ref)continue;put(tr>0?g+tr:r,calcFn(st.fn,[val(r,ci[0]),rv]))}}
    else if(ci.length>1){for(let r=g;r<end;r++)put(tr>0?g+tr:r,calcFn(st.fn,ci.map(c=>val(r,c))))}
    else if(ci.length===1){const a=[];for(let r=g;r<end;r++)a.push(val(r,ci[0]));put(g+Math.max(0,tr),calcFn(st.fn,a))}}}
  else{
   const ci=st.columns.map(c=>col(c,false)).filter(i=>i>=0);const tname=st.append?st.name:st.targetColumn;const ti=col(tname,true);
   for(let g=0;g<grid.length;g+=G){const end=Math.min(g+G,grid.length);if(end-g<G)break;
    const rows=st.specificRow>0?[g+st.specificRow-1].filter(r=>r<end):Array.from({length:end-g},(_,k)=>g+k);
    const vals=[];let failed=false;for(const r of rows)for(const c of ci){const v=grid[r][c];const t=v==null?'':String(v).trim();if(t==='')continue;if(t==='-Infinity'||t==='FAILED'){failed=true;continue}vals.push(t)}
    const target=g+Math.min(Math.max(0,st.targetRow-1),end-g-1);
    if(failed){grid[target][ti]='FAILED';continue}
    if(!evalConds(vals,st.conditions,st.any))continue;
    if(st.append){const cur=grid[target][ti];if(cur==null||cur==='')grid[target][ti]=st.text;else if(!String(cur).toUpperCase().trim().endsWith(st.text.toUpperCase().trim()))grid[target][ti]=cur+' '+st.text}
    else grid[target][ti]=st.text}}}
 const keep=cols.map((c,i)=>i>=n0||c.show!==false);
 return {...d,heads:heads.filter((_,i)=>keep[i]),cols:cols.filter((_,i)=>keep[i]),rows:d.rows.map((r,i)=>({...r,values:grid[i].filter((_,k)=>keep[k])}))}}

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

const kids=(n,t)=>Array.from(n?n.children:[]).filter(c=>c.localName===t);
const kid=(n,t)=>kids(n,t)[0];
const ktext=(n,t)=>{const k=kid(n,t);return k?k.textContent.trim():''};
function stepsToXml(steps){
 const tag=(t,v)=>`<${t}>${esc(v)}</${t}>`;
 const cmd=(name,row,col)=>`<rule-action><name>${esc(name[0])}</name><command><name>${esc(name[1])}</name><target-row>${row}</target-row><target-column>${esc(col)}</target-column></command></rule-action>`;
 const calc=steps.filter(x=>x.kind==='calc').map(x=>`<calculationStep><pre-condition><group-row>${x.groupRow}</group-row></pre-condition><calculation-rule>${tag('name',x.name)}${tag('description',x.description)}${tag('function',x.fn)}${x.columns.map(c=>tag('column',c)).join('')}${cmd(['',''],x.targetRow,x.targetColumn)}</calculation-rule>${tag('active',x.active)}${tag('referenceIndex',x.referenceIndex)}</calculationStep>`).join('');
 const an=steps.filter(x=>x.kind==='analysis').map(x=>`<analysisStep><pre-condition>${x.columns.map(c=>tag('columns',c)).join('')}<group-row>${x.groupRow}</group-row>${x.specificRow?`<specific-row>${x.specificRow}</specific-row>`:''}</pre-condition><analysis-rule>${tag('name',x.name)}${tag('description',x.description)}${x.conditions.map(c=>`<condition>${tag('left',c.left)}${tag('ncols',c.ncols)}${tag('operator',c.operator)}${tag('right',c.right)}${tag('logical',x.any?'OR':'AND')}</condition>`).join('')}${cmd([x.append?'APPEND':'SET',x.text],x.targetRow,x.append?x.name:x.targetColumn)}${tag('any-condition-rule',x.any)}</analysis-rule>${tag('active',x.active)}</analysisStep>`).join('');
 if(!calc&&!an)return '<step/>';
 return '<step>'+(calc?`<calculation calculate-type-count="${steps.filter(x=>x.kind==='calc').length}">${calc}</calculation>`:'')+(an?`<analysis>${an}</analysis>`:'')+'</step>'}
function stepsFromXml(prof){
 const out=[];
 for(const stp of prof.getElementsByTagName('step'))for(const c of kids(stp,'calculation'))for(const it of kids(c,'calculationStep')){
  const rule=kid(it,'calculation-rule');if(!rule)continue;const ra=kid(rule,'rule-action');const cm=kid(ra,'command');
  out.push({kind:'calc',name:ktext(rule,'name'),description:ktext(rule,'description'),fn:ktext(rule,'function').toLowerCase(),columns:kids(rule,'column').map(x=>x.textContent.trim()),
   groupRow:ktext(kid(it,'pre-condition'),'group-row'),referenceIndex:ktext(it,'referenceIndex'),targetColumn:ktext(cm,'target-column'),targetRow:ktext(cm,'target-row'),active:ktext(it,'active')!=='false'})}
 for(const stp of prof.getElementsByTagName('step'))for(const a of kids(stp,'analysis'))for(const it of kids(a,'analysisStep')){
  const rule=kid(it,'analysis-rule');if(!rule)continue;const pre=kid(it,'pre-condition');const ra=kid(rule,'rule-action');const cm=kid(ra,'command');
  out.push({kind:'analysis',name:ktext(rule,'name'),description:ktext(rule,'description'),columns:kids(pre,'columns').map(x=>x.textContent.trim()),groupRow:ktext(pre,'group-row'),specificRow:ktext(pre,'specific-row'),
   any:ktext(rule,'any-condition-rule')==='true',append:ktext(ra,'name').toUpperCase()==='APPEND',text:ktext(cm,'name'),targetColumn:ktext(cm,'target-column'),targetRow:ktext(cm,'target-row'),
   conditions:kids(rule,'condition').map(c=>({left:ktext(c,'left').toUpperCase(),ncols:ktext(c,'ncols'),operator:ktext(c,'operator').toUpperCase(),right:ktext(c,'right')})),active:ktext(it,'active')!=='false'})}
 return cleanSteps(out)}
function fromXml(text){
 const doc=parseSafe(text);const node=doc.getElementsByTagName('xmlReportSetting')[0];if(!node)throw Error(T('xmlInvalid'));
 const one=(n,t)=>t.getElementsByTagName(n)[0]?.textContent.trim();
 const content=one('content',node);if(!content)throw Error(T('xmlInvalid'));
 const prof=parseSafe(b64dec(content));
 const s={name:one('reportSettingName',node)||one('name',prof)||'Imported',description:one('comments',node)||'',numAlleles:parseInt(one('user-parameter',prof)||'2',10)||2,custom:[],columns:[],steps:stepsFromXml(prof)};
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
 const prof=`<?xml version="1.0" encoding="UTF-8"?>\n<process-profile xmlns="http://www.appliedbiosystems.com/GeneMapper" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="process-profile"><name>${esc(s.name)}</name><process-header>${cols}</process-header>${stepsToXml(s.steps||[])}<user-parameter>${s.numAlleles}</user-parameter></process-profile>`;
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

 // ----- Calculation / Analysis step editors -----
 const stepHeads=()=>{const h=s.columns.map(colHead);for(const x of (s.steps||[])){const t=x.kind==='analysis'&&x.append?x.name:x.targetColumn;if(t&&!h.includes(t))h.push(t)}return h};
 const stepDialog=(kind,cur)=>{
  const c=cur?clone(cur):kind==='calc'?{kind,name:'',description:'',active:true,fn:'sum',columns:[],groupRow:1,referenceIndex:0,targetColumn:'',targetRow:1}
   :{kind,name:'',description:'',active:true,columns:[],groupRow:1,specificRow:0,any:false,append:false,text:'',targetColumn:'',targetRow:1,conditions:[{left:'AT LEAST',ncols:1,operator:'EQUAL TO',right:''}]};
  const d2=dialog(T(kind==='calc'?'rsCalc':'rsAnalysis'),()=>{},[[T('ok'),()=>{
    try{c.name=c.name.trim();if(!c.name)throw Error(T('enterName'));
     if(!c.columns.length)throw Error(T('rsStepNoCols'));
     if(kind==='calc'&&!String(c.targetColumn).trim())throw Error(T('rsStepNoTarget'));
     if(kind==='analysis'&&!c.append&&!String(c.targetColumn).trim())throw Error(T('rsStepNoTarget'));
     if(kind==='analysis'&&!c.conditions.length)throw Error(T('rsStepNoCond'));
     const list=s.steps||(s.steps=[]);
     if((list.some(x=>x!==cur&&x.kind===kind&&x.name===c.name))){message(fmt('methodNameExists',c.name));return false}
     const clean=cleanSteps([c])[0];if(!clean)throw Error(T('rsStepNoTarget'));
     if(cur){Object.keys(cur).forEach(k=>delete cur[k]);Object.assign(cur,clean)}else list.push(clean);s.steps=cleanSteps(list);draw();return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]]);
  const row=(label,node)=>el('div',{class:'field'},el('label',{text:label}),node);
  const inp=(id,key,type='text',extra={})=>{const i=el('input',{type,id,value:String(c[key]??''),...extra});i.addEventListener('input',()=>{c[key]=type==='number'?(i.value===''?'':Number(i.value)):i.value});return i};
  const sel=(id,opts,key,lab)=>{const x=el('select',{id});for(const o of opts)x.append(new Option(lab?lab(o):o,o));x.value=c[key];x.addEventListener('change',()=>{c[key]=x.value;if(id==='stFn')drawD()});return x};
  const drawD=()=>{const b=d2.body;b.replaceChildren();b.classList.add('rsdlg','stepdlg');
   const heads=stepHeads();
   const colsBox=el('select',{id:'stCols',size:'6',class:'rslist'});c.columns.forEach((h,i)=>colsBox.append(new Option(h.startsWith(CONST)?T('rsConstant')+' '+h.slice(CONST.length):h,String(i))));
   const pick=el('select',{id:'stPick'});for(const h of heads)pick.append(new Option(h,h));
   const cst=el('input',{type:'number',id:'stConst',step:'any',class:'num',placeholder:T('rsConstant')});
   const btn=(text,fn,id)=>{const x=el('button',{class:'btn',type:'button',text,...(id?{id}:{})});x.addEventListener('click',fn);return x};
   const colsRow=el('div',{class:'btnrow'},pick,btn(T('add'),()=>{if(pick.value){c.columns.push(pick.value);drawD()}},'stAdd'));
   const constRow=kind==='calc'?el('div',{class:'btnrow'},cst,btn(T('rsAddConstant'),()=>{if(cst.value!==''&&Number.isFinite(Number(cst.value))){c.columns.push(CONST+Number(cst.value));drawD()}},'stAddConst')):null;
   const rm=btn(T('remove'),()=>{const i=parseInt(colsBox.value,10);if(i>=0){c.columns.splice(i,1);drawD()}},'stRemove');
   const tl=el('input',{type:'text',id:'stTarget',value:c.targetColumn,list:'stTargetList'});tl.addEventListener('input',()=>{c.targetColumn=tl.value});
   const dl=el('datalist',{id:'stTargetList'});for(const h of heads)dl.append(new Option(h,h));
   b.append(row(T('amName'),inp('stName','name')),row(T('amDescription'),inp('stDesc','description')));
   if(kind==='calc')b.append(row(T('rsFunction'),sel('stFn',FUNCS,'fn',f=>T('rsFn_'+f))),el('div',{class:'note',text:BINARY.has(c.fn)?T('rsFnBinary'):''}));
   b.append(el('div',{class:'cap',text:T('rsStepColumns')}),el('div',{class:'rsbody'},el('div',{class:'rscol'},colsBox,rm),el('div',{class:'rscol'},colsRow,constRow)));
   b.append(row(T('rsGroupRows'),inp('stGroup','groupRow','number',{min:'1',max:'1000',class:'num'})));
   if(kind==='calc')b.append(row(T('rsReferenceRow'),inp('stRef','referenceIndex','number',{min:'0',max:'1000',class:'num'})));
   else{b.append(row(T('rsSpecificRow'),inp('stSpec','specificRow','number',{min:'0',max:'1000',class:'num'})));
    const ct=el('table',{class:'g',id:'stConds'});ct.append(el('thead',{},el('tr',{},...['rsCondLeft','rsCondCount','rsCondOp','rsCondRight',''].map(k=>el('th',{text:k?T(k):''})))));
    const tb=el('tbody');c.conditions.forEach((cd,i)=>{
     const L=el('select',{'data-f':'left'});for(const o of LEFTS)L.append(new Option(T('rsLeft_'+o.replace(/ /g,'')),o));L.value=cd.left;L.addEventListener('change',()=>{cd.left=L.value});
     const N=el('input',{type:'number',min:'1',max:'1000',class:'num',value:String(cd.ncols),'data-f':'ncols'});N.addEventListener('input',()=>{cd.ncols=Number(N.value)});
     const O=el('select',{'data-f':'operator'});for(const o of OPS)O.append(new Option(T('rsOp_'+o.replace(/ /g,'')),o));O.value=cd.operator;O.addEventListener('change',()=>{cd.operator=O.value});
     const R=el('input',{type:'text',value:cd.right,'data-f':'right',title:T('rsRightHint')});R.addEventListener('input',()=>{cd.right=R.value});
     const X=btn('✕',()=>{c.conditions.splice(i,1);drawD()});
     tb.append(el('tr',{},el('td',{},L),el('td',{},N),el('td',{},O),el('td',{},R),el('td',{},X)))});
    ct.append(tb);
    const any=el('input',{type:'checkbox',id:'stAny'});any.checked=c.any;any.addEventListener('change',()=>{c.any=any.checked});
    b.append(el('div',{class:'cap',text:T('rsConditions')}),el('div',{class:'grid rscond'},ct),
     el('div',{class:'btnrow'},btn(T('add'),()=>{c.conditions.push({left:'AT LEAST',ncols:1,operator:'EQUAL TO',right:''});drawD()},'stAddCond'),el('label',{class:'chk'},any,document.createTextNode(' '+T('rsAnyCond')))));
    b.append(row(T('rsResultText'),inp('stText','text')));
    const ap=el('input',{type:'checkbox',id:'stAppend'});ap.checked=c.append;ap.addEventListener('change',()=>{c.append=ap.checked;drawD()});
    b.append(el('label',{class:'chk'},ap,document.createTextNode(' '+T('rsAppend'))))}
   if(!(kind==='analysis'&&c.append))b.append(row(T('rsTargetColumn'),tl),dl);
   b.append(row(T('rsTargetRow'),inp('stTRow','targetRow','number',{min:'1',max:'1000',class:'num'})))};
  drawD()};
 const stepsPane=kind=>{const list=(s.steps||[]).map((x,i)=>[x,i]).filter(([x])=>x.kind===kind);let sel=-1;
  const t=el('table',{class:'g',id:'stepsTable'});t.append(el('thead',{},el('tr',{},el('th',{text:T('rsShow')}),el('th',{text:T('amName').replace(':','')}),el('th',{text:kind==='calc'?T('rsFunction'):T('rsConditions')}),el('th',{text:T('rsTargetColumn')}))));
  const tb=el('tbody');list.forEach(([x,gi],k)=>{const cb=el('input',{type:'checkbox'});cb.checked=x.active;cb.addEventListener('change',()=>{x.active=cb.checked});cb.addEventListener('mousedown',e=>e.stopPropagation());
   const tr=el('tr',{class:sel===k?'sel':''},el('td',{class:'ic'},cb),el('td',{text:x.name}),el('td',{text:kind==='calc'?T('rsFn_'+x.fn):String(x.conditions.length)}),el('td',{text:kind==='analysis'&&x.append?x.name:x.targetColumn}));
   tr.addEventListener('mousedown',()=>{sel=k;tb.querySelectorAll('tr').forEach((r,j)=>r.classList.toggle('sel',j===k))});tr.addEventListener('dblclick',()=>stepDialog(kind,x));tb.append(tr)});
  t.append(tb);
  const btn=(text,fn,id)=>{const x=el('button',{class:'btn',type:'button',text,...(id?{id}:{})});x.addEventListener('click',fn);return x};
  const mv=dlt=>{if(sel<0)return;const j=sel+dlt;if(j<0||j>=list.length)return;const a=list[sel][1],b2=list[j][1];[s.steps[a],s.steps[b2]]=[s.steps[b2],s.steps[a]];draw()};
  return el('div',{class:'stepspane'},el('div',{class:'grid rsgrid stgrid'},t),el('div',{class:'btnrow'},btn(T('rsCreate'),()=>stepDialog(kind,null),'stNew'),btn(T('rsEdit'),()=>{if(sel>=0)stepDialog(kind,list[sel][0])},'stEdit'),
   btn(T('mgrDelete'),()=>{if(sel>=0){s.steps.splice(list[sel][1],1);draw()}},'stDelete'),btn('▲',()=>mv(-1),'stUp'),btn('▼',()=>mv(1),'stDown')),el('p',{class:'note',text:T('rsStepsNote')}))};
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('rsdlg');
  const name=el('input',{type:'text',id:'rsName',value:s.name});name.addEventListener('input',()=>{s.name=name.value});
  const desc=el('input',{type:'text',id:'rsDesc',value:s.description||''});desc.addEventListener('input',()=>{s.description=desc.value});
  const na=el('input',{type:'number',id:'rsAlleles',min:'1',max:'20',value:String(s.numAlleles),class:'num'});
  na.addEventListener('change',()=>{const v=Math.max(1,Math.min(20,parseInt(na.value,10)||1));s.numAlleles=v;s.columns=s.columns.filter(c=>c.index==null||c.index<v);draw()});
  const tabs=el('div',{class:'tabs dtabs'});
  for(const [id,key] of [['Sample','samples'],['Genotype','genotypes'],['Custom','rsCustom'],['Calc','rsCalc'],['Analysis','rsAnalysis']]){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key),'data-tab':id});t.addEventListener('mousedown',()=>{tab=id;availSel=null;draw()});tabs.append(t)}
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
   (tab==='Calc'||tab==='Analysis')?el('div',{},tabs,el('div',{class:'tabpage dpage'},stepsPane(tab==='Calc'?'calc':'analysis'))):
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

window.ReportSettings={applySteps,evalConds,cleanSteps,FUNCS,KEY,DEFAULTS,list,names,get,put,remove,validate,fromXml,toXml,openEditor,colHead,isAllele};
})();
