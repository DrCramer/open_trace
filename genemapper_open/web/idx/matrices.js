// Matrices tab of the OpenTrace Manager and Matrix Editor (ID-X: GeneMapper ID-X Manager > Matrices, MatrixMakerDiag).
// A matrix is created from 310 Genetic Analyzer runs of DS-32 (4-dye) / DS-33 (5-dye) matrix standards; it is used to
// multicomponent raw 310 sample data (Matrix column of the Samples table, MNF flag) and exported as an ABIF .mtx file.
(function(){
const KEY='opentrace.matrices.v1';
const LETTERS=['B','G','Y','R','O'];
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
const post=(url,body)=>Methods.post(url,body);
function load(){try{const v=JSON.parse(localStorage.getItem(KEY)||'null');if(v&&Array.isArray(v.matrices))return v}catch(e){}return {schema:1,matrices:[]}}
function store(db){try{localStorage.setItem(KEY,JSON.stringify(db));return true}catch(e){return false}}
function list(){return load().matrices}
function names(){return list().map(r=>r.matrix.name)}
function get(name){return list().find(r=>r.matrix.name===name)?.matrix||null}
// ID-X: name required, < 256 characters, no special characters; values 0..2; 4 or 5 dyes.
function validate(m){
 if(!m||typeof m.name!=='string'||!m.name.trim())throw Error(T('mxEnterName'));
 if(m.name.length>=256)throw Error(T('mxNameTooLong'));
 if(/[\\/:*?"<>|]/.test(m.name))throw Error(fmt('mxNameInvalid',m.name));
 const n=m.dyes;if(n!==4&&n!==5)throw Error(T('mxDyes'));
 if(!Array.isArray(m.values)||m.values.length!==n||m.values.some(r=>!Array.isArray(r)||r.length!==n))throw Error(T('mxCreateFirst'));
 m.values=m.values.map(r=>r.map(Number));if(m.values.flat().some(v=>!isFinite(v)||v<0||v>2))throw Error(T('mxValues'));
 m.description=String(m.description||'');return m}
function put(m,previous){validate(m);const db=load();const i=db.matrices.findIndex(r=>r.matrix.name===(previous??m.name));
 const row={matrix:clone(m),saved:new Date().toISOString()};if(i>=0)db.matrices[i]=row;else db.matrices.push(row);
 if(!store(db))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-matrices'))}
function remove(name){const db=load();db.matrices=db.matrices.filter(r=>r.matrix.name!==name);store(db);window.dispatchEvent(new Event('opentrace-matrices'))}
const b64=buf=>{let s='';const u=new Uint8Array(buf);for(let i=0;i<u.length;i+=0x8000)s+=String.fromCharCode(...u.subarray(i,i+0x8000));return btoa(s)};
const unb64=s=>{const bin=atob(s);const u=new Uint8Array(bin.length);for(let i=0;i<bin.length;i++)u[i]=bin.charCodeAt(i);return u};

// Minimal ABIF directory reader (public ABIF v1 layout) to tell 310 data from spectrally calibrated data at add time.
async function sniff(blob){
 try{const buf=await blob.arrayBuffer();const d=new DataView(buf);if(buf.byteLength<128||String.fromCharCode(...new Uint8Array(buf,0,4))!=='ABIF')return null;
  const count=d.getInt32(18),dir=d.getInt32(26);const tags={};
  for(let i=0;i<count&&dir+i*28+28<=buf.byteLength;i++){const p=dir+i*28;const name=String.fromCharCode(...new Uint8Array(buf,p,4));const num=d.getInt32(p+4);
   const type=d.getInt16(p+8),n=d.getInt32(p+12),size=d.getInt32(p+16);const off=size<=4?p+20:d.getInt32(p+20);
   if(['MODL','MCHN','HCFG','MTRD','MTRX'].includes(name)&&off>=0&&off+size<=buf.byteLength)tags[name+num]={type,n,size,bytes:new Uint8Array(buf,off,size)}}
  const txt=k=>{const t=tags[k];if(!t)return '';let b=t.bytes;if(t.type===18)b=b.subarray(1,1+b[0]);return String.fromCharCode(...b).replace(/\0+$/,'')};
  const all=['MODL1','MCHN1','HCFG1'].map(txt).join(' ');
  const is310=/310/.test(all)&&!/3100|3130/.test(all);
  const dim=tags.MTRD1?new DataView(tags.MTRD1.bytes.buffer,tags.MTRD1.bytes.byteOffset,2).getInt16(0):0;
  return {model:txt('MODL1').trim(),is310,embedded:(dim===4||dim===5)&&!!tags.MTRX1&&tags.MTRX1.n===dim*dim}}
 catch(e){return null}}

function resultTable(m,onEdit){
 const n=m.dyes;const t=el('table',{class:'mxres'});t.append(el('tr',{},el('th'),...LETTERS.slice(0,n).map(l=>el('th',{text:l}))));
 for(let r=0;r<n;r++){const tr=el('tr',{},el('th',{text:LETTERS[r]}));
  for(let c=0;c<n;c++){const v=m.values?.[r]?.[c];const i=el('input',{type:'text',class:'num','data-r':String(r),'data-c':String(c),value:v==null?'':Number(v).toFixed(4)});
   i.addEventListener('change',()=>onEdit(r,c,i.value));tr.append(el('td',{},i))}
  t.append(tr)}
 return t}

function openEditor(source,{title,onSave,restrict=null}={}){
 const m=clone(source);const previous=source.name;const files=m._files||[];delete m._files;
 let starts=(m._starts||[]).slice();delete m._starts;let points=m._points||100000;delete m._points;
 const d=dialog(title||T('mxEditor'),b=>{b.classList.add('mxdlg');
  const name=el('input',{type:'text',id:'mxName',value:m.name});name.addEventListener('input',()=>{m.name=name.value});
  const desc=el('input',{type:'text',id:'mxDesc',value:m.description||''});desc.addEventListener('input',()=>{m.description=desc.value});
  const dyes=el('select',{id:'mxDyes'});for(const k of [4,5])dyes.append(new Option(String(k),String(k)));dyes.value=String(m.dyes||4);
  const settings=el('fieldset',{},el('legend',{text:T('mxSettings')}));const result=el('fieldset',{},el('legend',{text:T('mxResult')}));
  const draw=()=>{const n=+dyes.value;settings.replaceChildren(el('legend',{text:T('mxSettings')}),el('div',{class:'note',text:T('mxSelectFiles')}));
   const grid=el('div',{class:'mxgrid'});grid.append(el('span'),el('span',{class:'h',text:T('mxFile')}),el('span',{class:'h',text:T('mxStartAt')}));
   for(let k=0;k<n;k++){if(starts[k]==null)starts[k]=1000;
    const inp=el('input',{type:'file',accept:'.fsa,.ab1',style:'display:none','data-dye':LETTERS[k]});
    const lbl=el('span',{class:'mxfile',id:'mxFile'+LETTERS[k],text:files[k]?.name||T('mxNoFile')});
    inp.addEventListener('change',()=>{if(inp.files[0]){files[k]=inp.files[0];lbl.textContent=files[k].name}});
    const btn=el('button',{class:'btn mxdye',type:'button',text:LETTERS[k],id:'mxBtn'+LETTERS[k]});btn.addEventListener('click',()=>inp.click());
    const st=el('input',{type:'text',class:'num',id:'mxStart'+LETTERS[k],value:String(starts[k])});st.addEventListener('change',()=>{starts[k]=parseInt(st.value,10)});
    grid.append(el('span',{},btn,inp),lbl,st)}
   const pts=el('input',{type:'text',class:'num',id:'mxPoints',value:String(points)});pts.addEventListener('change',()=>{points=parseInt(pts.value,10)});
   const create=el('button',{class:'btn',type:'button',id:'mxCreate',text:T('mxCreate')});create.addEventListener('click',()=>make(n));
   settings.append(grid,el('div',{class:'field'},el('label',{text:T('mxPoints')}),pts),el('div',{class:'btnrow'},create));
   result.replaceChildren(el('legend',{text:T('mxResult')}),m.values&&m.values.length===n?resultTable(m,edit):el('div',{class:'note',text:T('mxNoResult')}))};
  const edit=(r,c,v)=>{m.values[r][c]=v===''?NaN:Number(v)};
  async function make(n){
   for(let k=0;k<n;k++){if(!files[k])return message(fmt('mxNoFileFor',LETTERS[k]));if(!Number.isInteger(starts[k])||starts[k]<0)return message(T('mxNumbers'))}
   if(!Number.isInteger(points)||points<1)return message(T('mxNumbers'));
   const status=document.getElementById('statusText');if(status)status.textContent=T('mxCreating');
   try{const standards=[];for(let k=0;k<n;k++)standards.push({fsa_b64:b64(await files[k].arrayBuffer()),filename:files[k].name,start:starts[k],points});
    const r=await post('/api/matrix',{action:'make',dyes:n,standards});
    if(r.error){message(r.error);return}
    m.values=r.matrix;m.dyes=n;draw()}catch(e){message(e.message)}
   finally{if(status)status.textContent=''}}
  dyes.addEventListener('change',()=>{const n=+dyes.value;if(n!==m.dyes){m.dyes=n;m.values=null}draw()});
  b.append(el('div',{class:'field'},el('label',{text:T('mxName')}),name),el('div',{class:'field'},el('label',{text:T('mxDescription')}),desc),
   el('div',{class:'field'},el('label',{text:T('mxNumberOfDyes')}),dyes),settings,result);draw()},
  [[T('ok'),async()=>{try{if(restrict){message(T('scReadOnly'));return false}m.name=m.name.trim();
    if(!m.values||m.values.length!==m.dyes){message(T('mxCreateFirst'));return false}
    if(m.name!==previous&&get(m.name)){message(fmt('mxNameExists',m.name));return false}
    validate(m);const v=await post('/api/matrix',{action:'validate',matrix:m.values});if(v.error){message(v.error);return false}
    const isNew=!(previous&&get(previous));const ch=window.Sec?await Sec.change({map:'Matrix',attribute:isNew?'created':'modified',instance:m.name,old:isNew?'':previous,new:m.name,
     event:isNew?'Add Matrix':'Modify Matrix',esig:{group:'GM_MATRIX',action:isNew?'CREATE_MATRIX':'MODIFY_MATRIX'}}):{commit(){}};
    if(!ch)return false;onSave(m,previous);ch.commit();return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]]);
 if(restrict)d.root.querySelectorAll('.body input,.body select,.body button').forEach(x=>x.disabled=true);
 return d}

if(window.Methods)Methods.registerKind(()=>({id:'matrices',label:T('mgrMatrices'),allowEmpty:true,
 sec:{map:'Matrix',group:'GM_MATRIX',noun:'MATRIX',title:'Matrix',read:'READ_MATRIX',update:'UPDATE_MATRIX'},
 rows:()=>list().map(r=>({name:r.matrix.name,saved:r.saved,a:r.matrix.description||'',b:String(r.matrix.dyes)})),
 heads:[T('mxDescription').replace(':',''),T('mxNumberOfDyes').replace(':','')],get,put,remove,count:()=>list().length,
 create:async()=>{let n=1;while(get(`${T('mxUntitled')} ${n}`))n++;return {name:`${T('mxUntitled')} ${n}`,description:'',dyes:4,values:null}},
 edit:(obj,opts)=>openEditor(obj,opts),newTitle:T('mxEditor'),accept:'.mtx,.json',
 importFile:async f=>{if(/\.json$/i.test(f.name))return {obj:validate(JSON.parse(await f.text())),note:''};
  const r=await post('/api/matrix',{action:'import',mtx_b64:b64(await f.arrayBuffer()),filename:f.name});if(r.error)throw Error(r.error);
  return {obj:validate({name:f.name.replace(/\.[^.]+$/,''),description:r.comment||'',dyes:r.dyes,values:r.matrix}),note:''}},
 exportFile:async m=>{const r=await post('/api/matrix',{action:'export',matrix:m.values,comment:m.description||''});if(r.error)throw Error(r.error);
  return {name:m.name+'.mtx',blob:new Blob([unb64(r.mtx_b64)],{type:'application/octet-stream'})}},
 imported:T('mxImported'),confirmDelete:'mxConfirmDelete'}));

window.Matrices={KEY,LETTERS,list,names,get,put,remove,validate,openEditor,sniff};
})();
