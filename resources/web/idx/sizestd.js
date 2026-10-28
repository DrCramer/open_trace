// Size standard library, Size Standard Editor and GeneMapper SizeStandardContainer XML.
(function(){
const KEY='opentrace.sizestd.v1';
const DYES=['blue','green','yellow','red','orange','purple'];
// Fragment lists of the size standards shipped with ID-X 1.6 (published product facts).
const GS600=[60,80,100,114,120,140,160,180,200,214,220,240,250,260,280,300,314,320,340,360,380,400,414,420,440,460];
const DEFAULTS=[
 {name:'GS600_LIZ_(60-460)',dye:'orange',sizes:GS600,description:''},
 {name:'GS600_LIZ',dye:'orange',sizes:GS600.filter(x=>x>=80),description:''},
 {name:'GS600_LIZ_(80-400)',dye:'orange',sizes:GS600.filter(x=>x>=80&&x<=400),description:''},
 {name:'CE_G5_HID_GS500',dye:'orange',sizes:[75,100,139,150,160,200,300,340,350,400,450],description:''},
 {name:'CE_F_HID_GS500 (75-450)',dye:'red',sizes:[75,100,139,150,160,200,300,340,350,400,450],description:''},
 {name:'CE_F_HID_GS500 (75-400)',dye:'red',sizes:[75,100,139,150,160,200,300,340,350,400],description:''},
 // ID-X 1.6 Database Defaults: size standards for 3500 normalization
 {name:'GS600_LIZ+Normalization_(60-460)',dye:'orange',sizes:GS600,description:''},
 {name:'GS600_LIZ+Normalization_(80-400)',dye:'orange',sizes:GS600.filter(x=>x>=80&&x<=400),description:''},
];
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
function load(){try{const v=JSON.parse(localStorage.getItem(KEY)||'null');if(v&&Array.isArray(v.standards)&&v.standards.length){
  // add shipped standards that appeared in later versions (once)
  if((v.schema||1)<2){const now=new Date().toISOString();for(const s of DEFAULTS)if(!v.standards.some(r=>r.standard.name===s.name))v.standards.push({standard:clone(s),saved:now});
   v.schema=2;try{localStorage.setItem(KEY,JSON.stringify(v))}catch(e){}}
  return v}}catch(e){}
 const now=new Date().toISOString();return {schema:2,standards:DEFAULTS.map(s=>({standard:clone(s),saved:now}))}}
function store(db){try{localStorage.setItem(KEY,JSON.stringify(db));return true}catch(e){return false}}
function list(){return load().standards}
function names(){return list().map(r=>r.standard.name)}
function get(name){return list().find(r=>r.standard.name===name)?.standard||null}
function validate(s){
 if(!s||typeof s.name!=='string'||!s.name.trim()||s.name.length>240)throw Error(T('enterName'));
 if(!DYES.includes(s.dye))throw Error(T('stdDyeRequired'));
 const sizes=(s.sizes||[]).map(Number);
 if(sizes.length<4||sizes.length>128||sizes.some(x=>!isFinite(x)||x<=0))throw Error(T('stdSizesInvalid'));
 for(let i=1;i<sizes.length;i++)if(sizes[i]<=sizes[i-1])throw Error(T('stdSizesOrder'));
 s.sizes=sizes;s.description=String(s.description||'');return s}
function put(s,previous){validate(s);const db=load();const i=db.standards.findIndex(r=>r.standard.name===(previous??s.name));
 const row={standard:clone(s),saved:new Date().toISOString()};if(i>=0)db.standards[i]=row;else db.standards.push(row);
 if(!store(db))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-sizestd'))}
function remove(name){const db=load();db.standards=db.standards.filter(r=>r.standard.name!==name);store(db);window.dispatchEvent(new Event('opentrace-sizestd'))}

function fromXml(text){
 if(text.length>1048576||/<!DOCTYPE|<!ENTITY/i.test(text))throw Error(T('xmlInvalid'));
 const doc=new DOMParser().parseFromString(text,'application/xml');
 if(doc.getElementsByTagName('parsererror').length)throw Error(T('xmlInvalid'));
 const node=doc.getElementsByTagName('xmlSizeStandard')[0];if(!node)throw Error(T('xmlInvalid'));
 const one=n=>node.getElementsByTagName(n)[0]?.textContent.trim();
 const dyeIndex=parseInt(one('dyeIndex')||'5',10);
 const sizes=[...node.getElementsByTagName('sizeStdDefinition')].map(n=>Number(n.textContent));
 return validate({name:one('sizeStandardName')||'Imported',dye:DYES[dyeIndex-1]||'orange',sizes,description:''})}
function toXml(s){const esc=x=>String(x).replace(/&/g,'&amp;').replace(/</g,'&lt;');
 return '<?xml version="1.0" encoding="UTF-8"?>\n<SizeStandardContainer xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="SizeStandardContainer">'+
  `<appVersion>1.4</appVersion><fileVersion>1.0</fileVersion><xmlSizeStandard><sizeStandardName>${esc(s.name)}</sizeStandardName><userName>OpenTrace</userName>`+
  `<dyeIndex>${DYES.indexOf(s.dye)+1}</dyeIndex><type>1</type>`+s.sizes.map(x=>`<sizeStdDefinition>${Number(x).toFixed(1)}</sizeStdDefinition>`).join('')+
  s.sizes.map(()=>'<scanDefinition>0.0</scanDefinition>').join('')+'</xmlSizeStandard></SizeStandardContainer>\n'}

function openEditor(source,{title,onSave,restrict=null}={}){
 const s=clone(source);const previous=source.name;
 dialog(title||T('sizeStandardEditor').replace(/\.+$/,''),b=>{b.classList.add('stddlg');
  const name=el('input',{type:'text',id:'ssName',value:s.name});name.addEventListener('input',()=>{s.name=name.value});
  const desc=el('input',{type:'text',value:s.description||''});desc.addEventListener('input',()=>{s.description=desc.value});
  const dye=el('select',{id:'ssDye'});for(const d of DYES)dye.append(new Option(T('dye_'+d),d));dye.value=s.dye;dye.addEventListener('change',()=>{s.dye=dye.value});
  const sizes=el('textarea',{id:'ssSizes',rows:'10',cols:'18'});sizes.value=s.sizes.join('\n');
  sizes.addEventListener('input',()=>{s.sizes=sizes.value.split(/[\s,;]+/).filter(Boolean).map(Number)});
  b.append(el('div',{class:'field'},el('label',{text:T('amName')}),name),el('div',{class:'field'},el('label',{text:T('amDescription')}),desc),
   el('div',{class:'field'},el('label',{text:T('stdDye')}),dye),el('div',{class:'field'},el('label',{text:T('stdSizes')}),sizes))},
  [[T('ok'),async()=>{try{if(restrict){message(T('scReadOnly'));return false}s.name=s.name.trim();if(s.name!==previous&&get(s.name)){message(fmt('methodNameExists',s.name));return false}
    validate(s);const isNew=!(previous&&get(previous));const ch=window.Sec?await Sec.change({map:'SizeStandard',attribute:isNew?'created':'modified',instance:s.name,old:isNew?'':previous,new:s.name,
     event:isNew?'Add Size Standard':'Modify Size Standard',esig:{group:'GM_SIZE_STANDARD',action:isNew?'CREATE_SIZE_STANDARD':'MODIFY_SIZE_STANDARD'}}):{commit(){}};
    if(!ch)return false;onSave(s,previous);ch.commit();return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]]);
 if(restrict){const d=[...document.querySelectorAll('.dlg')].pop();d.querySelectorAll('.body input,.body select,.body textarea').forEach(x=>x.disabled=true)}}

if(window.Methods)Methods.registerKind(()=>({id:'sizestd',label:T('mgrSizeStandards'),sec:{map:'SizeStandard',group:'GM_SIZE_STANDARD',noun:'SIZE_STANDARD',title:'Size Standard',read:'READ_SIZE_STANDARD',update:'UPDATE_SIZE_STANDARD'},
 rows:()=>list().map(r=>({name:r.standard.name,saved:r.saved,a:T('dye_'+r.standard.dye),b:`${r.standard.sizes.length}: ${r.standard.sizes[0]}–${r.standard.sizes[r.standard.sizes.length-1]}`})),
 heads:[T('stdDye').replace(':',''),T('stdSizes').replace(':','')],get,put,remove,count:()=>list().length,
 create:async()=>{let n=1;while(get(`${T('stdUntitled')} ${n}`))n++;return {name:`${T('stdUntitled')} ${n}`,dye:'orange',sizes:GS600.slice(),description:''}},
 edit:(obj,opts)=>openEditor(obj,opts),newTitle:T('sizeStandardEditor').replace(/\.+$/,''),
 importText:async text=>({obj:/^\s*[{]/.test(text)?validate(JSON.parse(text)):fromXml(text),note:''}),
 exportText:async obj=>toXml(obj),imported:T('stdImported'),confirmDelete:'confirmDeleteStd'}));

window.SizeStd={KEY,DYES,list,names,get,put,remove,validate,fromXml,toXml,openEditor};
})();
