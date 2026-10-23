// Shared Panel Manager store: kits imported from GeneMapper text files.
// Parsed structures are the source of truth; GeneMapper-format text is
// regenerated from them for export and for building analysis kits on the server.
(function(){
const KEY='opentrace.kits.v1';
const EMPTY=()=>({schema:1,revision:0,kits:[]});

function load(){
 try{const raw=localStorage.getItem(KEY);if(!raw)return EMPTY();const db=JSON.parse(raw);
  if(!db||db.schema!==1||!Array.isArray(db.kits))return EMPTY();return db}catch(e){return EMPTY()}}
function save(db){
 db.revision=(db.revision||0)+1;
 try{localStorage.setItem(KEY,JSON.stringify(db));return true}catch(e){return false}}
const clone=x=>JSON.parse(JSON.stringify(x));

// ----- GeneMapper text writers (tab separated, CRLF) -----
const num=v=>{const s=String(v);return /[.eE]/.test(s)?s:s+'.0'};
const list=(a,trailing)=>a.length?(trailing?a.join(', ')+', ':a.join(',')):'none';
function panelsText(kit){
 const anyQc=kit.panels.some(p=>p.markers.some(m=>'internal_qc' in m));
 const rows=[['Version',kit.panelsVersion||'GMID-X v 1.4'],['Kit type:',kit.type||'MICROSATELLITE'],['Chemistry Kit',kit.name,kit.comment||'null']];
 for(const p of kit.panels){rows.push(['Panel',p.name,p.comment||'null']);
  for(const m of p.markers){const r=[m.name,m.dye,num(m.min_bp),num(m.max_bp),m.control_alleles.length?m.control_alleles.join(','):'none',String(m.repeat),m.option||'none',String(!!m.y_marker)];
   if(anyQc)r.push(String(!!m.internal_qc));r.push(list(m.ladder_alleles,true));rows.push(r)}}
 return rows.map(r=>r.join('\t')).join('\r\n')+'\r\n'}
function binsText(kit,set){
 const rows=[['Version',set.version||'GMID-X v 1.2'],['Chemistry Kit',kit.name],['BinSet Name',set.name]];
 for(const [panel,markers] of Object.entries(set.panels)){rows.push(['Panel Name',panel]);
  for(const [marker,bins] of Object.entries(markers)){rows.push(['Marker Name',marker]);
   for(const b of bins){const r=[b.label,String(b.center_bp),String(b.left_bp),String(b.right_bp)];if(b.virtual)r.push('virtual');rows.push(r)}}}
 return rows.map(r=>r.join('\t')).join('\r\n')+'\r\n'}
function stutterText(kit){
 const st=kit.stutter;if(!st)return null;
 const rows=[['Version',st.version||'SSFv1.2'],['Chemistry Kit',kit.name]];
 for(const [panel,markers] of Object.entries(st.panels)){rows.push(['Panel Name',panel]);
  for(const [marker,items] of Object.entries(markers)){rows.push(['Marker Name',marker]);
   for(const s of items)rows.push([String(s.ratio),String(s.min_bp),String(s.max_bp),s.direction==='plus'?'Plus':'Minus',''])}}
 return rows.map(r=>r.join('\t')).join('\r\n')+'\r\n'}

// ----- merge server parse results into the store -----
function addPanels(db,doc){
 const added=[];
 for(const k of doc.kits){
  let kit=db.kits.find(x=>x.name===k.name);
  if(!kit){kit={name:k.name,type:doc.kit_type||'MICROSATELLITE',comment:'',panelsVersion:doc.version,panels:[],binSets:[],stutter:null};db.kits.push(kit)}
  for(const p of k.panels){const i=kit.panels.findIndex(x=>x.name===p.name);const copy={name:p.name,comment:'',markers:clone(p.markers)};
   if(i>=0)kit.panels[i]=copy;else kit.panels.push(copy);added.push(p.name)}
  kit.panelsVersion=doc.version}
 return added}
function addBinSet(db,kitName,doc){
 const kit=db.kits.find(x=>x.name===(doc.kit&&db.kits.some(k=>k.name===doc.kit)?doc.kit:kitName));
 if(!kit)throw Error('kit');
 const name=doc.bin_set||kit.name+'_Bins';const set={name,version:doc.version,panels:clone(doc.panels)};
 const i=kit.binSets.findIndex(x=>x.name===name);if(i>=0)kit.binSets[i]=set;else kit.binSets.push(set);
 return {kit:kit.name,binSet:name}}
function setStutter(db,kitName,doc){
 const kit=db.kits.find(x=>x.name===kitName);if(!kit)throw Error('kit');
 kit.stutter={version:doc.version,panels:clone(doc.panels)};return kit.name}

// Panel choices for the Samples table: one per kit/panel (bins come from the method's bin set).
function choices(db){
 const out=[];
 for(const kit of db.kits)for(const p of kit.panels)
  out.push({key:[kit.name,p.name].join('\u0001'),kit:kit.name,panel:p.name,label:p.name,
   binSets:kit.binSets.filter(s=>s.panels[p.name]).map(s=>s.name)});
 return out}
// Bin set: the analysis method's bin set when the kit has it for this panel, else the first one.
function request(db,key,binSetName){
 const [kitName,panel]=key.split('\u0001');
 const kit=db.kits.find(x=>x.name===kitName);if(!kit||!kit.panels.some(p=>p.name===panel))return null;
 const usable=kit.binSets.filter(s=>s.panels[panel]);
 const set=usable.find(s=>s.name===binSetName)||usable[0];if(!set)return null;
 return {panels:panelsText(kit),bins:binsText(kit,set),stutter:stutterText(kit),panel,binSet:set.name,
  binSetSubstituted:!!binSetName&&set.name!==binSetName}}

window.KitStore={KEY,load,save,clone,panelsText,binsText,stutterText,addPanels,addBinSet,setStutter,choices,request};
})();
