// Table Settings: visible columns, allele count and sort keys for the Samples
// and Genotypes tables; editor and GeneMapper TableSettingContainer XML.
(function(){
const KEY='opentrace.tables.v1';
// [id, xml field (TableSettingContainer), header, width, kind]. Headers are the
// column names of the original tables; they stay the same in both languages.
const SAMPLE_COLS=[
 ['status','status','Status',48,'icon'],['sampleFile','sampleFile','Sample File',170,'text'],['sampleName','sampleName','Sample Name',130,'text'],
 ['sampleID','sampleID','Sample ID',90,'text'],['comments','comments','Comment',110,'text'],['sampleType','sampleType','Sample Type',150,'edit'],
 ['analysisMethod','analysisMethod','Analysis Method',130,'edit'],['panel','panel','Panel',150,'edit'],['sizeStandard','sizeStandard','Size Standard',130,'edit'],
 ['matrix','matrix','Matrix',70,'text'],['runName','runName','Run Name',150,'text'],['instrumentType','instrumentType','Instrument Type',90,'text'],
 ['instrumentID','instrumentID','Instrument ID',90,'text'],['runDate','runDate','Run Date',130,'text'],['plate','plate','Plate Name',110,'text'],
 ['well','well','Well',44,'text'],['lane','lane','Capillary',58,'num'],['sampleFileNorm','sampleFileNorm','SFN',44,'text'],
 ['SE','sampleAlleleEdit','SE',36,'pqv'],['SQO','sizingQualityInvalid','SQO',38,'pqv'],['ARNM','arnm','ARNM',44,'pqv'],
 ['SFNF','fileNotFound','SFNF',42,'pqv'],['MNF','matrixNotFound','MNF',38,'pqv'],['SNF','sizeStdNotFound','SNF',38,'pqv'],
 ['SOS','offscale','SOS',38,'pqv'],['SQ','sizeStdQuality','SQ',38,'pqv'],['SSPK','spikeSamplePqvQuality','SSPK',42,'pqv'],
 ['MIX','mixedSamplePqvQuality','MIX',38,'pqv'],['OMR','omrSamplePqvQuality','OMR',42,'pqv'],['customControl','customControl','Custom Control',100,'text'],
 ['CGQ','smplGenoTypeQuality','CGQ',42,'pqv'],['specimenCategory','specimenType','Specimen Category',140,'edit'],['profileId','profileId','Profile ID',90,'text'],
 ...[1,2,3,4,5,6,7,8,9].map(i=>['UD'+i,'userDefinedColumn'+i,'UD'+i,60,'text'])];
const GENO_COLS=[
 ['sampleFile','sampleFile','Sample File',160,'text'],['sampleName','sampleName','Sample Name',120,'text'],['sampleID','sampleID','Sample ID',80,'text'],
 ['runName','runName','Run Name',140,'text'],['panel','panel','Panel',130,'text'],['marker','marker','Marker',80,'text'],['dye','dyeColor','Dye',36,'text'],
 ['allele','allele','Allele',56,'allele'],['size','peakSize','Size',60,'allele'],['height','peakHeight','Height',58,'allele'],
 ['area','peakArea','Peak Area',66,'allele'],['dataPoint','peakScan','Data Point',62,'allele'],['aeReason','alleleComment','AE Reason For Change',120,'text'],
 ['MEC',null,'MEC',38,'pqv'],['ADO',null,'ADO',38,'pqv'],['ME',null,'ME',34,'pqv'],
 ['OS','offscale','OS',34,'pqv'],['BIN','binProblem','BIN',34,'pqv'],['PHR','peakHeightRatio','PHR',38,'pqv'],['LPH','absoluteHeight','LPH',38,'pqv'],
 ['MPH','highPeakHeight','MPH',38,'pqv'],['SPK','spikeMarkerPqv','SPK',36,'pqv'],['AN','numAlleleError','AN',34,'pqv'],['ACC','amelCrossCheck','ACC',36,'pqv'],
 ['BD','broadPeak','BD',34,'pqv'],['CC','controlConcordance','CC',34,'pqv'],['OVL','overlap','OVL',36,'pqv'],['GQ','alleleConfidence','GQ',40,'pqv'],
 ['PLI','partialLocusIndicator','PLI',34,'text'],['profileId',null,'Profile ID',90,'text'],
 ...[1,2,3,4,5,6,7,8,9].map(i=>['UD'+i,'userDefinedColumn'+i,'UD'+i,60,'text'])];
const cols=list=>list.map(([id,xml,head,width,kind])=>({id,xml,head,width,kind}));
const CATALOG={samples:cols(SAMPLE_COLS),genotypes:cols(GENO_COLS)};

// "3500 Data Analysis_6Dye" visibility of the shipped defaults (evidence/tables).
const DEFAULT={name:'Data Interpretation',
 samples:{visible:['status','sampleFile','sampleName','sampleType','analysisMethod','panel','sizeStandard','sampleFileNorm','ARNM','SOS','SQ','SSPK','MIX','OMR','customControl','CGQ'],
  sort:[{col:'sampleFile',desc:false}]},
 genotypes:{visible:['sampleName','marker','allele','OS','BIN','PHR','LPH','MPH','SPK','AN','ACC','BD','CC','GQ'],numAlleles:2,
  sort:[{col:'sampleName',desc:false},{col:'dye',desc:false},{col:'marker',desc:false}]}};
// ---------- column filters (TableSettingContainer filterCriteria; ProfileTableConstants lists of GMp.jar) ----------
// op 0 = Show All Records. Text columns: op 1 = Show Records Containing: operand1.
// Quality columns use FilterCriteria.OPERATION_QUALITY_* codes: pass 1, check (warning) 2, low (fail) 3, override 4.
const Q={override:[4,'Show Override'],pass:[1,'Show Pass'],check:[2,'Show Check'],low:[3,'Show Low Quality']};
const qual=keys=>({type:'enum',options:keys.map(k=>({op:Q[k][0],key:k,label:Q[k][1]}))});
const en=(pairs)=>({type:'enum',options:pairs.map(([key,label],i)=>({op:i+1,key,label}))});
const SPECIMEN=['No Export','Alleged Father','Alleged Mother','Biological Child','Biological Father','Biological Mother','Biological Sibling','Convicted Offender',
 'Deduced Victim Known','Elimination, Known','Forensic Mixture','Forensic, Unknown','Maternal Relative','Missing Person','Other','Paternal Relative','Population',
 'Proficiency','Suspect, Known','Unidentified Person','Victim, Known'];
const FILTERS={
 samples:{status:en([['analyzed','Analyzed'],['unanalyzed','Unanalyzed'],['changed','Analysis Setting Changed']]),
  sampleType:en([['sample','Sample'],['positive','Positive Control'],['ladder','Allelic Ladder'],['negative','Negative Control']]),
  specimenCategory:en(SPECIMEN.map(x=>[x,x])),sampleFileNorm:en([['yes','Yes'],['no','No'],['na','N/A']]),
  SQ:qual(['override','pass','check','low']),CGQ:qual(['override','pass','check','low']),SOS:qual(['pass','check','low']),
  SSPK:qual(['pass','check']),MIX:qual(['pass','check']),OMR:qual(['pass','check']),ARNM:qual(['pass','check']),
  SFNF:en([['pass','Found'],['low','Not Found']]),MNF:en([['pass','Found'],['low','Not Found']]),SNF:en([['pass','Found'],['low','Not Found']]),
  SE:en([['check','Yes'],['pass','No']]),SQO:en([['check','Yes'],['pass','No']])},
 genotypes:{dye:en([['B','B'],['G','G'],['Y','Y'],['R','R'],['P','P'],['O','O']]),GQ:qual(['override','pass','check','low']),
  ME:en([['check','Yes'],['pass','No']]),MEC:en([['check','Yes'],['pass','No']]),ADO:en([['check','Yes'],['pass','No']])}};
const CONTAINS={type:'contains',options:[{op:1,label:'Show Records Containing:'}]};
function filterSpec(kind,id){const c=CATALOG[kind].find(x=>x.id===id);if(!c)return null;return FILTERS[kind][id]||(c.kind==='pqv'?qual(['pass','check','low']):CONTAINS)}
// raw: the row's key (status, type, flag, dye letter) for enum filters, the displayed text for text filters.
function matches(kind,id,f,raw){if(!f||!f.op)return true;const sp=filterSpec(kind,id);if(!sp)return true;
 if(sp.type==='contains')return String(raw??'').toLowerCase().includes(String(f.text||'').toLowerCase());
 const o=sp.options.find(x=>x.op===f.op);return !o||o.key===raw}
function activeFilters(kind,s){const out=[];const fl=(s||active())[kind].filters||{};for(const [id,f] of Object.entries(fl))if(f&&f.op)out.push([id,f]);return out}
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);

function load(){try{const v=JSON.parse(localStorage.getItem(KEY)||'null');if(v&&Array.isArray(v.settings)&&v.settings.length)return v}catch(e){}
 return {schema:1,active:DEFAULT.name,settings:[{setting:clone(DEFAULT),saved:new Date().toISOString()}]}}
function store(db){try{localStorage.setItem(KEY,JSON.stringify(db));return true}catch(e){return false}}
function list(){return load().settings}
function get(name){return list().find(r=>r.setting.name===name)?.setting||null}
function active(){const db=load();return get(db.active)||db.settings[0].setting}
function setActive(name){const db=load();db.active=name;store(db);window.dispatchEvent(new Event('opentrace-tables'))}
function validate(s){
 if(!s||typeof s.name!=='string'||!s.name.trim()||s.name.length>240)throw Error(T('enterName'));
 for(const kind of ['samples','genotypes']){const t=s[kind];if(!t||!Array.isArray(t.visible))throw Error('table setting: '+kind);
  const ids=new Set(CATALOG[kind].map(c=>c.id));t.visible=t.visible.filter(x=>ids.has(x));
  t.sort=(t.sort||[]).filter(x=>x&&ids.has(x.col)).slice(0,3).map(x=>({col:x.col,desc:!!x.desc}));
  const fl={};for(const [id,f] of Object.entries(t.filters||{})){const sp=ids.has(id)&&filterSpec(kind,id);if(!sp||!f)continue;const op=Number(f.op)|0;
   if(!op)continue;if(sp.type==='contains'){const text=String(f.text||'').slice(0,200);if(text)fl[id]={op:1,text}}else if(sp.options.some(o=>o.op===op))fl[id]={op}}
  t.filters=fl}
 const n=Number(s.genotypes.numAlleles);if(!Number.isInteger(n)||n<1||n>20)throw Error(T('numAllelesRange'));
 s.genotypes.numAlleles=n;return s}
function put(s,previous){validate(s);const db=load();const i=db.settings.findIndex(r=>r.setting.name===(previous??s.name));
 const row={setting:clone(s),saved:new Date().toISOString()};if(i>=0)db.settings[i]=row;else db.settings.push(row);
 if(previous&&db.active===previous)db.active=s.name;if(!store(db))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-tables'))}
function remove(name){const db=load();db.settings=db.settings.filter(r=>r.setting.name!==name);if(db.active===name)db.active=db.settings[0]?.setting.name;
 store(db);window.dispatchEvent(new Event('opentrace-tables'))}

// ---------- GeneMapper TableSettingContainer XML ----------
function fromXml(text){
 if(text.length>1048576)throw Error('XML > 1 MiB');if(/<!DOCTYPE|<!ENTITY/i.test(text))throw Error('DTD');
 const doc=new DOMParser().parseFromString(text,'application/xml');
 if(doc.getElementsByTagName('parsererror').length)throw Error(T('xmlInvalid'));
 const prof=doc.getElementsByTagName('xmlProjectTableProfile')[0];if(!prof)throw Error(T('xmlInvalid'));
 const child=(n,name)=>[...n.children].find(c=>c.localName===name);
 const s={name:(child(prof,'profileName')?.textContent||'Imported').trim(),samples:{visible:[],sort:[]},genotypes:{visible:[],numAlleles:2,sort:[]}};
 const read=(kind,secName)=>{const sec=child(prof,secName);if(!sec)return;
  for(const c of CATALOG[kind]){if(!c.xml)continue;const n=child(sec,c.xml+'Col')||child(sec,c.xml);
   const v=n&&child(n,'visible');if(v&&v.textContent.trim()==='true')s[kind].visible.push(c.id);
   const fc=n&&child(n,'filterCriteria');if(fc){const op=parseInt(child(fc,'operation')?.textContent||'0',10)||0;if(op){(s[kind].filters=s[kind].filters||{})[c.id]={op,text:(child(fc,'operand1')?.textContent||'').trim()}}}}};
 read('samples','sampleProfile');read('genotypes','markerProfile');
 const mp=child(prof,'markerProfile');const na=mp&&child(mp,'numAllelesToShow');if(na)s.genotypes.numAlleles=Math.max(1,Math.min(20,parseInt(na.textContent,10)||2));
 const byHead=kind=>Object.fromEntries(CATALOG[kind].map(c=>[c.head,c.id]));
 const sorts=(keys,orders,kind)=>{const ks=[...prof.children].filter(c=>c.localName===keys).map(c=>c.textContent.trim());
  const os=[...prof.children].filter(c=>c.localName===orders).map(c=>c.textContent.trim());const map=byHead(kind);
  return ks.map((k,i)=>map[k]?{col:map[k],desc:os[i]==='1'}:null).filter(Boolean)};
 s.samples.sort=sorts('sampleColsSort','sampleSortColOrder','samples');s.genotypes.sort=sorts('markerColsSort','markerSortOrder','genotypes');
 return validate(s)}
function esc(x){return String(x).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')}
function toXml(s){
 const sec=(kind,name)=>{const vis=new Set(s[kind].visible);let x=`<${name}>`;
  const fl=s[kind].filters||{};
  for(const c of CATALOG[kind])if(c.xml){const f=fl[c.id]||{};x+=`<${c.xml}Col><visible>${vis.has(c.id)}</visible><filterCriteria><operation>${f.op||0}</operation><operand1>${esc(f.text||'')}</operand1><operand2></operand2></filterCriteria></${c.xml}Col>`}
  if(kind==='genotypes')x+=`<numAllelesToShow>${s.genotypes.numAlleles}</numAllelesToShow>`;return x+`</${name}>`};
 const sorts=(kind,keys,orders)=>{const head=Object.fromEntries(CATALOG[kind].map(c=>[c.id,c.head]));const rows=[0,1,2].map(i=>s[kind].sort[i]);
  return rows.map(r=>`<${keys}>${esc(r?head[r.col]:'None')}</${keys}>`).join('')+rows.map(r=>`<${orders}>${r&&r.desc?1:0}</${orders}>`).join('')};
 return '<?xml version="1.0" encoding="UTF-8"?>\n<TableSettingContainer xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="TableSettingContainer">'+
  `<appVersion>1.4</appVersion><fileVersion>1.0</fileVersion><xmlProjectTableProfile><profileName>${esc(s.name)}</profileName><owner>OpenTrace</owner>`+
  sec('genotypes','markerProfile')+sorts('samples','sampleColsSort','sampleSortColOrder')+sorts('genotypes','markerColsSort','markerSortOrder')+
  sec('samples','sampleProfile')+'</xmlProjectTableProfile></TableSettingContainer>\n'}

// ---------- editor ----------
const colTitle=(kind,c)=>{const k='colT_'+c.id;const t=T(k);return t===k?c.head:t};
function openEditor(source,{title,onSave}={}){
 const s=clone(source);const previous=source.name;let tab='samples';
 const d=dialog(title||T('tableSettingEditor').replace(/\.+$/,''),b=>{b.classList.add('tsdlg')},
  [[T('ok'),()=>{try{s.name=(s.name||'').trim();if(s.name!==previous&&get(s.name)){message(fmt('methodNameExists',s.name));return false}
    validate(s);onSave(s,previous);return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();
  const name=el('input',{type:'text',id:'tsName',value:s.name});name.addEventListener('input',()=>{s.name=name.value});
  const tabs=el('div',{class:'tabs dtabs'});
  for(const [id,key] of [['samples','samples'],['genotypes','genotypes']]){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key)});t.addEventListener('mousedown',()=>{tab=id;draw()});tabs.append(t)}
  const page=el('div',{class:'tabpage dpage'});const t=s[tab];const vis=new Set(t.visible);
  t.filters=t.filters||{};
  const grid=el('table',{class:'g colfilter'});grid.append(el('thead',{},el('tr',{},...['tfShow','tfColumn','tfFiltering','tfContent'].map(k=>el('th',{text:T(k)})))));
  const gb=el('tbody');grid.append(gb);
  for(const c of CATALOG[tab]){const cb=el('input',{type:'checkbox','data-col':c.id});cb.checked=vis.has(c.id);
   cb.addEventListener('change',()=>{const order=CATALOG[tab].map(x=>x.id);const set=new Set(t.visible);cb.checked?set.add(c.id):set.delete(c.id);t.visible=order.filter(x=>set.has(x))});
   const sp=filterSpec(tab,c.id);const f=t.filters[c.id]||{op:0,text:''};
   const sel=el('select',{'data-filter':c.id});const tl=x=>{const k='tf_'+x.replace(/[^A-Za-z]/g,''),v=T(k);return v===k?x:v};sel.append(new Option(tl('Show All Records'),'0'));for(const o of sp.options)sel.append(new Option(tl(o.label),String(o.op)));sel.value=String(f.op||0);
   const txt=el('input',{type:'text','data-filter-text':c.id,value:f.text||''});txt.disabled=sp.type!=='contains'||sel.value==='0';
   const upd=()=>{const op=Number(sel.value);txt.disabled=sp.type!=='contains'||!op;if(!op)delete t.filters[c.id];else t.filters[c.id]=sp.type==='contains'?{op,text:txt.value}:{op}};
   sel.addEventListener('change',upd);txt.addEventListener('input',upd);
   gb.append(el('tr',{},el('td',{class:'c'},cb),el('td',{title:colTitle(tab,c),text:c.head+(colTitle(tab,c)!==c.head?' — '+colTitle(tab,c):'')}),el('td',{},sel),el('td',{},txt)))}
  const sortBox=el('fieldset',{},el('legend',{text:T('sortBy')}));
  for(let i=0;i<3;i++){const sel=el('select',{'data-sort':String(i)});sel.append(new Option(T('sortNone'),''));for(const c of CATALOG[tab])sel.append(new Option(c.head,c.id));
   const cur=t.sort[i];sel.value=cur?cur.col:'';const dir=el('select');dir.append(new Option(T('ascending'),'0'),new Option(T('descending'),'1'));dir.value=cur&&cur.desc?'1':'0';
   const upd=()=>{const rows=[...sortBox.querySelectorAll('[data-sort]')].map(x=>({col:x.value,desc:x.nextSibling.value==='1'})).filter(x=>x.col);t.sort=rows};
   sel.addEventListener('change',upd);dir.addEventListener('change',upd);sortBox.append(el('div',{class:'field'},el('label',{text:T(i?'thenBy':'sortFirst')}),el('span',{class:'inl'},sel,dir)))}
  page.append(el('fieldset',{},el('legend',{text:T('showColumns')}),el('div',{class:'grid tfgrid'},grid)),sortBox);
  if(tab==='genotypes'){const n=el('input',{type:'text',class:'num',id:'tsAlleles',value:String(t.numAlleles)});n.addEventListener('change',()=>{t.numAlleles=Number(n.value)});
   page.append(el('div',{class:'field'},el('label',{text:T('numAlleles')}),n))}
  b.append(el('div',{class:'field'},el('label',{text:T('amName')}),name),tabs,page)};
 draw();return d}

if(window.Methods)Methods.registerKind(()=>({id:'tables',label:T('mgrTables'),
 rows:()=>list().map(r=>({name:r.setting.name,saved:r.saved,a:String(r.setting.samples.visible.length),b:String(r.setting.genotypes.visible.length)})),
 heads:[T('tsSampleCols'),T('tsGenoCols')],get,put,remove,count:()=>list().length,
 create:async()=>{const f=clone(DEFAULT);let n=1;while(get(`${T('tsUntitled')} ${n}`))n++;f.name=`${T('tsUntitled')} ${n}`;return f},
 edit:(obj,opts)=>openEditor(obj,opts),newTitle:T('tableSettingEditor'),
 importText:async text=>({obj:/^\s*[{]/.test(text)?validate(JSON.parse(text)):fromXml(text),note:''}),
 exportText:async obj=>toXml(obj),imported:T('tableImported'),confirmDelete:'confirmDeleteTable'}));

window.TableSettings={KEY,CATALOG,DEFAULT,list,get,put,remove,active,setActive,validate,fromXml,toXml,openEditor,filterSpec,matches,activeFilters,
 factory:()=>clone(DEFAULT)};
})();
