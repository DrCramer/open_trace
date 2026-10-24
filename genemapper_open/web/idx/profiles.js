// Profile Manager, Add Profile, Profile Comparison, CODIS Export Manager and Export Table for CODIS.
// Profiles and CODIS lists are per browser profile (ID-X keeps them in its database).
(function(){
const PKEY='opentrace.profiles.v1',CKEY='opentrace.codis.v1';
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
let ctx=null; // set by project.js: {post(url,data), samples(), plot(files), user()}
const read=(k,d)=>{try{const v=JSON.parse(localStorage.getItem(k)||'null');return v??d}catch(e){return d}};
const write=(k,v)=>{try{localStorage.setItem(k,JSON.stringify(v));return true}catch(e){return false}};

// ---------- profile store ----------
function profiles(){const v=read(PKEY,null);return v&&Array.isArray(v.profiles)?v.profiles:[]}
function saveProfiles(list){if(!write(PKEY,{schema:1,profiles:list}))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-profiles'))}
const customControls=()=>profiles().filter(p=>p.type==='Custom Control');
function getProfile(id){return profiles().find(p=>p.id===id)||null}
// Add profiles from samples: an existing Profile ID only gets new markers (ID-X behaviour).
function addFromSamples(list,type){const db=profiles();const report=[];
 for(const s of list){if(!s.profileId){report.push([s.name,T('pfNoProfileId')]);continue}
  if(!Object.keys(s.markers).length){report.push([s.name,T('pfNoAlleles')]);continue}
  let p=db.find(x=>x.id===s.profileId);
  if(!p){p={id:s.profileId,type,markers:{},dye:{},size:{},added:new Date().toISOString(),source:s.file};db.push(p);
   Object.assign(p.markers,s.markers);Object.assign(p.dye,s.dye||{});Object.assign(p.size,s.size||{});report.push([s.name,fmt('pfAdded',s.profileId,Object.keys(s.markers).length)]);continue}
  let n=0;for(const [m,a] of Object.entries(s.markers))if(!p.markers[m]){p.markers[m]=a;if(s.dye?.[m])p.dye=Object.assign(p.dye||{},{[m]:s.dye[m]});n++}
  report.push([s.name,n?fmt('pfMarkersAdded',s.profileId,n):fmt('pfNothingNew',s.profileId)])}
 saveProfiles(db);
 dialog(T('pfAddTitle'),b=>{const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},el('th',{text:T('colSampleName')}),el('th',{text:T('pfStatus')}))));
  const tb=el('tbody');for(const [n,st] of report)tb.append(el('tr',{},el('td',{text:n}),el('td',{text:st})));t.append(tb);b.append(el('div',{class:'grid mgrgrid'},t))})}

function download(name,text,type){const url=URL.createObjectURL(new Blob([text],{type:type||'text/plain'}));const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
function pickFile(accept){return new Promise(res=>{const i=el('input',{type:'file',accept,style:'display:none'});i.addEventListener('change',async()=>{const f=i.files[0];i.remove();res(f?await f.text():null)});document.body.append(i);i.click()})}

// ---------- Profile Manager ----------
function profileManager(){let sel=null;const open=new Set();
 const d=dialog(T('profileManager').replace(/\.+$/,''),()=>{},[[T('closeWin'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('pmgrdlg');
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...['',T('pfProfileId'),T('pfType'),T('pfMarkers'),T('pfAddedOn')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');
  for(const p of profiles()){const exp=open.has(p.id);const tw=el('span',{class:'twisty',text:exp?'−':'+'});
   const tr=el('tr',{class:p.id===sel?'sel':''},el('td',{class:'ic'},tw),el('td',{text:p.id}),el('td',{text:T(p.type==='Custom Control'?'pfCustom':'pfLab')}),el('td',{class:'num',text:String(Object.keys(p.markers).length)}),el('td',{text:p.added?new Date(p.added).toLocaleString():''}));
   tw.addEventListener('mousedown',e=>{e.stopPropagation();exp?open.delete(p.id):open.add(p.id);draw()});
   tr.addEventListener('mousedown',()=>{sel=p.id;tb.querySelectorAll('tr.prow').forEach(x=>x.classList.toggle('sel',x===tr))});tr.classList.add('prow');tb.append(tr);
   if(exp)for(const [m,a] of Object.entries(p.markers))tb.append(el('tr',{class:'sub'},el('td'),el('td',{text:'   '+m}),el('td',{text:p.dye?.[m]||''}),el('td',{text:a.join(', ')}),el('td')))}
  t.append(tb);
  const btn=(text,fn,id)=>{const x=el('button',{class:'btn',type:'button',text,id});x.addEventListener('click',fn);return x};
  b.append(el('div',{class:'grid mgrgrid'},t),el('div',{class:'btnrow'},
   btn(T('pfExpandAll'),()=>{if(open.size)open.clear();else profiles().forEach(p=>open.add(p.id));draw()},'pfExpand'),
   btn(T('mgrImport'),async()=>{const text=await pickFile('.txt,.csv,.tsv');if(text==null)return;
    try{const r=await ctx.post('/api/profiles',{action:'parse',text});const db=profiles();let added=0,merged=0;
     for(const p of r.profiles){const cur=db.find(x=>x.id===p.id);if(!cur){db.push({...p,added:new Date().toISOString(),source:'import'});added++}
      else{for(const [m,a] of Object.entries(p.markers))if(!cur.markers[m]){cur.markers[m]=a;merged++}}}
     saveProfiles(db);draw();message(fmt('pfImported',added,merged))}catch(e){message(e.message)}},'pfImport'),
   btn(T('mgrExport'),async()=>{if(!profiles().length)return message(T('pfNone'));try{const r=await ctx.post('/api/profiles',{action:'export',profiles:profiles()});download('Profiles.txt',r.text)}catch(e){message(e.message)}},'pfExport'),
   btn(T('mgrDelete'),async()=>{if(!sel)return message(T('selectRow'));if(!(await Methods.ask(fmt('pfConfirmDelete',sel))))return;saveProfiles(profiles().filter(p=>p.id!==sel));sel=null;draw()},'pfDelete')),
   el('div',{class:'note',text:T('pfNote')}))};
 draw();return d}

// ---------- Profile Comparison ----------
const pqvBlue=a=>el('span',{class:'pm-match',text:a});
function allelesCell(list,matched,bold){const td=el('td');list.forEach((a,i)=>{if(i)td.append(', ');const n=matched.includes(a)?pqvBlue(a):el('span',{text:a});if(bold)n.style.fontWeight='bold';td.append(n)});return td}
function profileComparison(){
 const all=ctx.samples();const eligible=all.filter(s=>s.eligible);
 let tab='concordance',threshold=80,ignore=read('opentrace.pcIgnore',''),results={},openRows=new Set(),selected=new Set(eligible.map(s=>s.id)),selRow=null;
 const comments={};
 const units=()=>eligible.map(s=>({id:s.id,markers:s.markers}));
 const filesOf=ids=>ids.map(id=>all.find(s=>s.id===id)?.file).filter(Boolean);
 const run=async mode=>{try{const req={action:'compare',mode,samples:units(),threshold,ignore:ignore.split(/[,\s]+/).filter(Boolean)};
   if(mode==='lab_reference'){req.profiles=profiles().filter(p=>p.type!=='Custom Control');req.selected=[...selected]}
   if(mode==='control')req.profiles=customControls();
   results[mode]=await ctx.post('/api/profiles',req);openRows.clear();selRow=null;draw()}catch(e){message(e.message)}};
 const d=dialog(T('profileComparison').replace(/\.+$/,''),()=>{},[
  [T('displayPlots'),()=>{const r=selRow;if(!r)return message(T('selectRow')),false;ctx.plot(filesOf([...(r.reference_members||[]),...(r.comparison_members||[])]));return false}],
  [T('exportBtn'),async()=>{const r=results[tab];if(!r||!r.rows)return message(T('pcNothing')),false;
    const nm=id=>all.find(s=>s.id===id)?.name||id;const rows=r.rows.map((x,i)=>({...x,reference:nm(x.reference),comparison:nm(x.comparison),comment:comments[tab+i]||''}));try{const o=await ctx.post('/api/profiles',{action:'export_csv',rows});download(`Profile Comparison - ${tab}.csv`,o.text,'text/csv')}catch(e){message(e.message)}return false}],
  [T('closeWin'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('pcdlg');
  const tabs=el('div',{class:'tabs dtabs'});
  for(const [id,key] of [['concordance','pcConcordance'],['samples','pcSamples'],['lab_reference','pcLabRef'],['control','pcControl']]){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key),'data-tab':id});t.addEventListener('mousedown',()=>{tab=id;selRow=null;openRows.clear();draw();if(id==='concordance'&&!results.concordance)run('concordance')});tabs.append(t)}
  const page=el('div',{class:'tabpage dpage'});
  page.append(el('div',{class:'note',text:fmt('pcEligible',eligible.length,all.length)}));
  if(tab==='concordance'){const r=results.concordance;const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...['',T('pcGroup'),T('samples')].map(h=>el('th',{text:h})))));const tb=el('tbody');
   for(const g of r?.groups||[]){const exp=openRows.has(g.id);const tw=el('span',{class:'twisty',text:exp?'−':'+'});
    const tr=el('tr',{},el('td',{class:'ic'},tw),el('td',{text:T(g.kind==='mixed'?'pcMixedGroup':'pcSingleGroup')+' '+g.id.replace(/\D+/g,'')}),el('td',{class:'num',text:String(g.members.length)}));
    tw.addEventListener('mousedown',()=>{exp?openRows.delete(g.id):openRows.add(g.id);draw()});tr.addEventListener('dblclick',()=>ctx.plot(filesOf(g.members)));tb.append(tr);
    if(exp)for(const m of g.members)tb.append(el('tr',{class:'sub'},el('td'),el('td',{text:'   '+(all.find(s=>s.id===m)?.name||m)}),el('td')))}
   t.append(tb);page.append(el('div',{class:'grid pcgrid'},t));if(r&&!r.groups.length)page.append(el('p',{text:T('pcNoGroups')}))}
  else{const thr=el('input',{type:'number',id:'pcThreshold',min:'50',max:'100',value:String(threshold),class:'num'});thr.addEventListener('change',()=>{threshold=Math.max(50,Math.min(100,Number(thr.value)||80));thr.value=threshold});
   const ig=el('input',{type:'text',id:'pcIgnore',value:ignore,placeholder:'F, A'});ig.addEventListener('change',()=>{ignore=ig.value;write('opentrace.pcIgnore',ignore)});
   const go=el('button',{class:'btn',type:'button',text:T('pcCompare'),id:'pcCompareBtn'});go.addEventListener('click',()=>run(tab));
   page.append(el('div',{class:'pcbar'},el('label',{text:T('pcThreshold')}),thr,el('label',{text:T('pcIgnoreLabels')}),ig,go));
   if(tab==='lab_reference'){const box=el('div',{class:'pcsel'});for(const s of eligible){const cb=el('input',{type:'checkbox','data-id':s.id});cb.checked=selected.has(s.id);cb.addEventListener('change',()=>{cb.checked?selected.add(s.id):selected.delete(s.id)});box.append(el('label',{class:'chk'},cb,document.createTextNode(' '+s.name)))}
    page.append(el('div',{class:'cap',text:fmt('pcLabRefNote',profiles().filter(p=>p.type!=='Custom Control').length)}),box)}
   if(tab==='control')page.append(el('div',{class:'cap',text:fmt('pcControlNote',customControls().length)}));
   const r=results[tab];const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...['',T('pcReference'),T('pcComparisonProfile'),T('pcMatch'),T('pcComment')].map(h=>el('th',{text:h})))));const tb=el('tbody');
   const nameOf=id=>all.find(s=>s.id===id)?.name||id;
   (r?.rows||[]).forEach((x,i)=>{const exp=openRows.has(i);const tw=el('span',{class:'twisty',text:exp?'−':'+'});const cm=el('input',{type:'text',value:comments[tab+i]||''});cm.addEventListener('input',()=>{comments[tab+i]=cm.value});cm.addEventListener('mousedown',e=>e.stopPropagation());
    const tr=el('tr',{class:'prow'+(selRow===x?' sel':'')},el('td',{class:'ic'},tw),el('td',{text:nameOf(x.reference)}),el('td',{text:nameOf(x.comparison)}),el('td',{class:'num',text:x.percent.toFixed(1)}),el('td',{class:'cust'},cm));
    tw.addEventListener('mousedown',e=>{e.stopPropagation();exp?openRows.delete(i):openRows.add(i);draw()});tr.addEventListener('mousedown',()=>{selRow=x;tb.querySelectorAll('tr.prow').forEach(y=>y.classList.toggle('sel',y===tr))});tb.append(tr);
    if(exp)for(const [m,v] of Object.entries(x.markers)){const sub=el('tr',{class:'sub'},el('td'),el('td',{text:'   '+m}));sub.append(allelesCell(v.reference,v.matched,true),allelesCell(v.comparison,v.matched,false),el('td'));tb.append(sub)}});
   t.append(tb);page.append(el('div',{class:'grid pcgrid'},t));if(r&&!r.rows.length)page.append(el('p',{text:fmt('pcNoResults',threshold)}))}
  b.append(tabs,page)};
 draw();run('concordance');return d}

// ---------- CODIS ----------
function codisConfig(){const v=read(CKEY,{})||{};return {sources:Array.isArray(v.sources)?v.sources:[],destinations:Array.isArray(v.destinations)?v.destinations:[],
 specimenTypes:Array.isArray(v.specimenTypes)&&v.specimenTypes.length?v.specimenTypes:null,markersText:typeof v.markersText==='string'?v.markersText:null,kitsText:typeof v.kitsText==='string'?v.kitsText:null,version:v.version==='3.3'?'3.3':'3.2'}}
function saveCodis(c){write(CKEY,c);window.dispatchEvent(new Event('opentrace-codis'))}
let catalog=null;
async function codisCatalog(){if(!catalog){const c=codisConfig();catalog=await ctx.post('/api/codis',{action:'config',markers_text:c.markersText,kits_text:c.kitsText})}return catalog}
function specimenTypes(){return codisConfig().specimenTypes||catalog?.default_specimen_types||['No Export']}
function codisManager(){codisCatalog().then(cat=>{const c=codisConfig();if(!c.specimenTypes)c.specimenTypes=[...cat.default_specimen_types];let tab='sources';
 const d=dialog(T('codisManager').replace(/\.+$/,''),()=>{},[[T('ok'),()=>{saveCodis(c);catalog=null;return true},true],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('codisdlg');
  const tabs=el('div',{class:'tabs dtabs'});for(const [id,key] of [['sources','cdSources'],['destinations','cdDestinations'],['specimenTypes','cdSpecimenTypes'],['files','cdFiles']]){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key),'data-tab':id});t.addEventListener('mousedown',()=>{tab=id;draw()});tabs.append(t)}
  const page=el('div',{class:'tabpage dpage'});
  if(tab==='files'){const mk=Object.keys(cat.markers).length;
   const imp=(key,label)=>{const x=el('button',{class:'btn',type:'button',text:label});x.addEventListener('click',async()=>{const t=await pickFile('');if(t==null)return;
     try{await ctx.post('/api/codis',{action:'config',[key==='markersText'?'markers_text':'kits_text']:t});c[key]=t;draw()}catch(e){message(e.message)}});return x};
   const reset=el('button',{class:'btn',type:'button',text:T('cdReset')});reset.addEventListener('click',()=>{c.markersText=null;c.kitsText=null;draw()});
   page.append(el('p',{text:fmt('cdFilesNote',mk,cat.kits.length)}),el('p',{class:'note',text:(c.markersText?T('cdCustomMarkers'):T('cdShippedMarkers'))+' '+(c.kitsText?T('cdCustomKits'):T('cdShippedKits'))}),
    el('div',{class:'btnrow'},imp('markersText',T('cdImportMarkers')),imp('kitsText',T('cdImportKits')),reset))}
  else{const list=c[tab];const sel=el('select',{size:'12',class:'rslist',id:'cdList'});for(const v of list)sel.append(new Option(v,v));
   const input=tab==='specimenTypes'?(()=>{const s=el('select',{id:'cdNew'});for(const v of cat.specimen_categories.filter(x=>!list.includes(x)))s.append(new Option(v,v));return s})():el('input',{type:'text',id:'cdNew',maxlength:'64'});
   const add=el('button',{class:'btn',type:'button',text:T('cdAdd'),id:'cdAdd'});add.addEventListener('click',()=>{const v=String(input.value||'').trim();if(!v||list.includes(v))return;if(/[|;]/.test(v))return message(T('cdBadChars'));list.push(v);draw()});
   const del=el('button',{class:'btn',type:'button',text:T('mgrDelete'),id:'cdDel'});del.addEventListener('click',()=>{const v=sel.value;if(!v||v==='No Export')return;c[tab]=list.filter(x=>x!==v);draw()});
   page.append(el('div',{class:'rsbody'},sel,el('div',{class:'rscol'},input,add,del)))}
  b.append(tabs,page)};
 draw()}).catch(e=>message(e.message))}
async function exportCodis(){
 let cat;try{cat=await codisCatalog()}catch(e){return message(e.message)}
 const all=ctx.samples();if(!all.some(s=>s.analyzed))return message(T('cdEmpty'));
 const list=all.filter(s=>s.category&&s.category!=='No Export');
 if(!list.length)return message(T('cdNoCategories'));
 const c=codisConfig();
 dialog(T('exportCodis').replace(/\.+$/,''),b=>{b.classList.add('ecdlg');
  const ver=el('select',{id:'cdVersion'});ver.append(new Option('CMF 3.2 (.xml)','3.2'),new Option('CMF 3.3 (.xml)','3.3'));ver.value=c.version;
  const src=el('select',{id:'cdSource'});for(const v of c.sources)src.append(new Option(v,v));
  const dst=el('select',{id:'cdDest'});for(const v of c.destinations)dst.append(new Option(v,v));
  b.append(el('div',{class:'field'},el('label',{text:T('cdExportAs')}),ver),el('fieldset',{},el('legend',{text:T('cdLabIds')}),
    el('div',{class:'field'},el('label',{text:T('cdSource')}),src),el('div',{class:'field'},el('label',{text:T('cdDestination')}),dst)),
   el('div',{class:'field'},el('label',{text:T('userName')}),el('input',{type:'text',id:'cdUser',value:ctx.user()})),
   el('div',{class:'field'},el('label',{text:T('fileName')}),el('input',{type:'text',id:'cdName',value:'CODIS_'+new Date().toISOString().slice(0,10)})),
   el('p',{class:'note',text:fmt('cdSummary',list.length)+(c.sources.length&&c.destinations.length?'':' '+T('cdNeedLabs'))}))},
  [[T('exportBtn'),async b=>{const version=b.querySelector('#cdVersion').value;c.version=version;saveCodis(c);
    const user=b.querySelector('#cdUser').value.trim();ctx.setUser(user);
    try{const r=await ctx.post('/api/codis',{action:'export',version,source_lab:b.querySelector('#cdSource').value,destination_ori:b.querySelector('#cdDest').value,user,
      markers_text:c.markersText,kits_text:c.kitsText,samples:list.map(s=>s.codis)});
     download((b.querySelector('#cdName').value.trim()||'CODIS')+'.xml',r.xml,'application/xml');
     message(fmt('cdDone',r.specimens,r.samples)+(r.skipped_markers.length?'\n'+T('cdSkipped')+'\n'+r.skipped_markers.join(', '):''));return true}
    catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]])}

window.Profiles={init(c){ctx=c},profiles,getProfile,customControls,addFromSamples,profileManager,profileComparison,codisManager,exportCodis,specimenTypes,codisCatalog};
})();
