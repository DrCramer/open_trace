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
// Merge incoming profiles into the store (ID-X ProfileManager): an existing Profile ID only gets markers it does not have yet;
// when a marker it already has carries different alleles the profile is rejected as a whole and listed with the conflicting markers.
const sameAlleles=(x,y)=>[...x].map(String).sort().join('\u0001')===[...y].map(String).sort().join('\u0001');
function mergeProfiles(incoming,db,{overwrite=false}={}){const success=[],errors=[];let added=0,merged=0;
 for(const p of incoming){let cur=db.find(x=>x.id===p.id);
  if(!cur){db.push({...clone(p),added:new Date().toISOString()});success.push(p.id);added++;continue}
  const conflicts=Object.keys(p.markers).filter(m=>cur.markers[m]&&!sameAlleles(cur.markers[m],p.markers[m]));
  if(conflicts.length&&!overwrite){errors.push({id:p.id,detail:conflicts.join(', '),conflicts});continue}
  for(const [m,al] of Object.entries(p.markers)){if(!cur.markers[m]||conflicts.includes(m)){cur.markers[m]=al;merged++;
    if(p.dye?.[m])(cur.dye=cur.dye||{})[m]=p.dye[m];if(p.size?.[m]!=null)(cur.size=cur.size||{})[m]=p.size[m]}}
  success.push(p.id)}
 return {success,errors,added,merged}}
// "Add Profile Results" window: successes and errors in two tables.
function resultsDialog(success,errors,skipped=[]){
 if(!success.length&&!errors.length&&!skipped.length)return message(T('pfResNone'));
 return dialog(T('pfResTitle'),b=>{b.classList.add('pfresdlg');
  const tbl=(heads,rows,id)=>{const t=el('table',{class:'g',id});t.append(el('thead',{},el('tr',{},...heads.map(h=>el('th',{text:h})))));const tb=el('tbody');for(const r of rows)tb.append(el('tr',{},...r.map(x=>el('td',{text:x}))));t.append(tb);return el('div',{class:'grid pfgrid'},t)};
  if(success.length)b.append(el('p',{text:T('pfResOk')}),tbl([T('pfProfileId')],success.map(x=>[x]),'pfOkTable'));
  if(errors.length||skipped.length)b.append(el('p',{class:'err',text:T('pfResError')}),tbl([T('pfResErrId'),T('pfResConflicts')],[...errors.map(e=>[e.id,e.detail]),...skipped],'pfErrTable'))},
  [[T('closeWin'),()=>true,true]])}
// Add profiles from samples. Samples without Profile ID or alleles, and samples with off-ladder (OL) alleles, are skipped.
function addFromSamples(list,type){const db=profiles();const skipped=[];const incoming=[];
 for(const s of list){if(!s.profileId){skipped.push([s.name,T('pfNoProfileId')]);continue}
  if(!Object.keys(s.markers).length){skipped.push([s.name,T('pfNoAlleles')]);continue}
  if(Object.values(s.markers).some(a=>a.some(x=>String(x).toUpperCase()==='OL'))){skipped.push([s.name,T('pfHasOl')]);continue}
  const ex=incoming.find(x=>x.id===s.profileId);
  if(ex){for(const [m,a] of Object.entries(s.markers))if(!ex.markers[m]){ex.markers[m]=a;if(s.dye?.[m])ex.dye[m]=s.dye[m];if(s.size?.[m]!=null)ex.size[m]=s.size[m]}continue}
  incoming.push({id:s.profileId,type,markers:{...s.markers},dye:{...(s.dye||{})},size:{...(s.size||{})},source:s.file})}
 const r=mergeProfiles(incoming,db);if(r.success.length)saveProfiles(db);
 resultsDialog(r.success,r.errors,skipped)}

function download(name,text,type){const url=URL.createObjectURL(new Blob([text],{type:type||'text/plain'}));const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
function pickFile(accept){return new Promise(res=>{const i=el('input',{type:'file',accept,style:'display:none'});i.addEventListener('change',async()=>{const f=i.files[0];i.remove();res(f?await f.text():null)});document.body.append(i);i.click()})}

// ---------- Profile Manager ----------
function importDialog(){return new Promise(res=>{let type='Lab Reference',ftype='tab';
 dialog(T('pfImportTitle'),b=>{b.classList.add('pfimpdlg');
  const radio=(name,value,label,cur,set)=>{const r=el('input',{type:'radio',name,value,id:`pf_${name}_${value.replace(/\W/g,'')}`});r.checked=cur===value;r.addEventListener('change',()=>{if(r.checked)set(value)});return el('label',{class:'chk'},r,document.createTextNode(' '+label))};
  b.append(el('fieldset',{},el('legend',{text:T('pfChooseType')}),radio('ptype','Lab Reference',T('pfLab'),type,v=>type=v),radio('ptype','Custom Control',T('pfCustom'),type,v=>type=v)),
   el('fieldset',{},el('legend',{text:T('pfChooseFileType')}),radio('ftype','tab',T('pfFileTab'),ftype,v=>ftype=v),radio('ftype','csv',T('pfFileCsv'),ftype,v=>ftype=v)))},
  [[T('ok'),()=>{res({type,ftype});return true},true],[T('cancel'),()=>{res(null);return true}]])})}
function profileManager(){let sel=null;const open=new Set();
 const d=dialog(T('profileManager').replace(/\.+$/,''),()=>{},[[T('closeWin'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('pmgrdlg');
  const t=el('table',{class:'g',id:'pfTree'});t.append(el('thead',{},el('tr',{},...['',T('pfIdMarker'),T('pfType'),T('pfAlleles')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');
  for(const p of profiles()){const exp=open.has(p.id);const tw=el('span',{class:'twisty',text:exp?'−':'+'});
   const tr=el('tr',{class:'prow'+(p.id===sel?' sel':''),title:p.added?T('pfAddedOn')+': '+new Date(p.added).toLocaleString():''},el('td',{class:'ic'},tw),el('td',{text:p.id}),el('td',{text:T(p.type==='Custom Control'?'pfCustom':'pfLab')}),el('td'));
   tw.addEventListener('mousedown',e=>{e.stopPropagation();exp?open.delete(p.id):open.add(p.id);draw()});
   tr.addEventListener('mousedown',()=>{sel=p.id;tb.querySelectorAll('tr.prow').forEach(x=>x.classList.toggle('sel',x===tr))});
   tr.addEventListener('dblclick',()=>{exp?open.delete(p.id):open.add(p.id);draw()});tb.append(tr);
   if(exp)for(const [m,a] of Object.entries(p.markers))tb.append(el('tr',{class:'sub'},el('td'),el('td',{text:'   '+m}),el('td',{text:p.dye?.[m]||''}),el('td',{text:a.join(', ')})))}
  t.append(tb);
  const btn=(text,fn,id,title)=>{const x=el('button',{class:'btn',type:'button',text,id,...(title?{title}:{})});x.addEventListener('click',fn);return x};
  b.append(el('div',{class:'grid mgrgrid'},t),el('div',{class:'btnrow'},
   btn(T('pfExpandAll'),()=>{profiles().forEach(p=>open.add(p.id));draw()},'pfExpand',T('pfExpandTip')),
   btn(T('pfCollapseAll'),()=>{open.clear();draw()},'pfCollapse',T('pfCollapseTip')),
   btn(T('mgrImport'),async()=>{const o=await importDialog();if(!o)return;const text=await pickFile('.txt,.csv,.tsv');if(text==null)return;
    const first=text.split('\n')[0]||'';const tabs=(first.match(/\t/g)||[]).length,commas=(first.match(/,/g)||[]).length;
    if((o.ftype==='tab'&&commas>tabs)||(o.ftype==='csv'&&tabs>commas))return message(T('pfBadFormat'));
    try{const r=await ctx.post('/api/profiles',{action:'parse',text});if(!r.profiles.length)return message(T('pfBadFormat'));
     const incoming=r.profiles.map(p=>({...p,type:o.type,source:'import'}));const db=profiles();
     let res=mergeProfiles(incoming,db);
     if(res.errors.length){const list=res.errors.map(e=>`${e.id}: ${e.detail}`).join('\n');
      if(await Methods.ask(fmt('pfOverwrite',list))){const again=mergeProfiles(res.errors.map(e=>incoming.find(p=>p.id===e.id)),db,{overwrite:true});res={success:[...res.success,...again.success],errors:[]}}}
     if(res.success.length)saveProfiles(db);draw();
     resultsDialog(res.success,res.errors);
     }catch(e){message(e.message)}},'pfImport'),
   btn(T('mgrExport'),async()=>{if(!profiles().length)return message(T('pfNone'));try{const r=await ctx.post('/api/profiles',{action:'export',profiles:profiles()});download('Profiles.txt',r.text)}catch(e){message(e.message)}},'pfExport'),
   btn(T('mgrDelete'),async()=>{if(!sel)return message(T('pfSelectProfile'));if(!(await Methods.ask(fmt('pfConfirmDelete',sel))))return;saveProfiles(profiles().filter(p=>p.id!==sel));sel=null;draw()},'pfDelete')),
   el('div',{class:'note',text:T('pfNote')}))};
 draw();return d}

// ---------- Profile Comparison ----------
const pqvBlue=a=>el('span',{class:'pm-match',text:a});
function allelesCell(list,matched,bold){const td=el('td');list.forEach((a,i)=>{if(i)td.append(', ');const n=matched.includes(a)?pqvBlue(a):el('span',{text:a});if(bold)n.style.fontWeight='bold';td.append(n)});return td}
// Profile Comparison (ID-X ProfileConcordanceDlg): Sample Concordance, Sample Comparison, Lab Reference Comparison, Control/QC Comparison.
// Calculation on the server in ID-X mode ("idx": true); layout, messages, export and Display Plot follow the decompiled ID-X windows.
const pcSplit=s=>String(s).split(','); // ID-X splits GenotypeMarker.c() ("a, b") on commas and keeps the leading blanks
const pcCsv=rows=>rows.map(r=>r.map(v=>v==null?'':'"'+String(v).replace(/"/g,'""')+'"').join(',')).join('\n')+'\n'; // opencsv CSVWriter: every non-null field quoted
const pcPct=v=>{const t=Number(v).toFixed(1);return (t.startsWith('0.')?t.slice(1):t)+'%'}; // DecimalFormat("###.0")
// Comparison alleles in the layout of the ID-X export: those missing from the reference are written in [brackets].
function pcCompText(refAlleles,compAlleles){const set=new Set((refAlleles||[]).map(a=>String(a).trim()));
 const parts=pcSplit((compAlleles||[]).join(', '));return parts.map((p,i)=>(refAlleles==null?p.trim()!=='':!(set.has(p.trim())||p.trim()===''))?'['+p+']':p).join(',')}
function profileComparison(){
 const all=ctx.samples();const eligible=all.filter(s=>s.eligible);
 let tab='concordance',threshold=80,ignore=read('opentrace.pcIgnore',''),results={},selected=new Set(eligible.map(s=>s.id));
 const comments={},checks=new Map(),openSamples=new Set(),ran={}; // checks: checkbox key -> sample files
 const byId=id=>all.find(s=>s.id===id);
 const labelOf=s=>s.label||[s.well||'',s.file,s.name].join(' | ');
 const fullLabel=s=>[s.well||'',s.file,s.name,s.runDate||''].join(' | ');
 const full=s=>{const pm=s.panelMarkers||[];const o={};for(const m of pm)o[m]=s.markers[m]||[];for(const [m,a] of Object.entries(s.markers))if(!(m in o))o[m]=a;return o};
 const units=()=>eligible.map(s=>({id:s.id,label:labelOf(s),markers:full(s)}));
 const filesOf=ids=>ids.map(id=>byId(id)?.file).filter(Boolean);
 if(all.some(s=>s.hasOl))message(T('pcOlWarn'),T('pcOlWarnTitle'));
 const thrOk=()=>Number.isInteger(threshold)&&threshold>=50&&threshold<=100;
 const run=async mode=>{if(mode!=='concordance'&&!thrOk()){message(fmt('pcInvalidValue',50,100),T('pcInvalidTitle'));return}
  try{const req={action:'compare',idx:true,mode,samples:units(),threshold,ignore:ignore.split(/[,\s]+/).filter(Boolean)};
   if(mode==='lab_reference'){req.profiles=profiles().filter(p=>p.type!=='Custom Control');req.selected=[...selected]}
   if(mode==='control')req.profiles=customControls();
   results[mode]=await ctx.post('/api/profiles',req);ran[mode]=true;draw();
   if(mode!=='concordance'&&!results[mode].rows.length)message(T('pcNoMatchesMsg'),T('pcNoMatchesTitle'))}catch(e){message(e.message)}};
 const exportCsv=()=>{const r=results[tab];const lines=[];
  if(tab==='concordance'){const g=r?.groups||[];if(!g.length)return message(T('pcNoDataExport'),T('exportBtn'));
   for(const kind of ['single','mixed']){const list=g.filter(x=>x.kind===kind);if(!list.length)continue;
    lines.push([kind==='single'?'Single Source':'Mixed Source','100% Concordance']);
    for(const x of list){lines.push([x.name,'']);lines.push(['Samples','']);for(const m of x.members){const s=byId(m);lines.push([s?fullLabel(s):m,''])}
     const ms=Object.entries(x.markers);lines.push(ms.map(e=>e[0]));lines.push(ms.map(e=>e[1].join(', ')))}}
  }else{if(!r||!r.rows||!r.rows.length||!thrOk())return message(T('pcNoDataExport'),T('exportBtn'));
   lines.push(['Threshold ',threshold+'%']);const panels=pcPanels(r.rows);let first=true;
   for(const p of panels){if(!first)lines.push(['','']);first=false;
    const names=[...p.markerNames];const rm=p.rows[0].markers;
    if(tab==='samples')lines.push([p.sectionText.replace(/ /g,''),'']);
    else lines.push([p.kind==='group'?'Project sample/source-group used as reference to calculate percent match':(tab==='control'?'Custom Control':'Lab Reference')+' profile used as reference to calculate percent match','']),lines.push([]);
    const hdr=[null,null,...names];const ref=[p.reference+' (Reference)',' % Match',...names.map(n=>{const m=p.refMarkers[n];return m?m.join(', '):''})];
    lines.push(hdr);lines.push(ref);
    for(const x of p.rows){const row=[x.comparison,pcPct(x.percent),...names.map(()=>undefined)];
     names.forEach((n,i)=>{const m=x.markers[n];if(!m)return;
      if(m.reference.length||x.markers[n].shared)row[i+2]=pcCompText(m.reference,m.comparison);
      else if(m.comparison.length)row[i+2]=pcCompText(null,m.comparison)});
     lines.push(row)}
    if(tab==='samples')lines.push(['Comments',comments[p.key]??''])}}
  download(tab==='concordance'?'Sample Concordance.csv':`Profile Comparison - ${tab}.csv`,pcCsv(lines),'text/csv')};
 // rows -> panels (one panel per reference as in ID-X)
 function pcPanels(rows){const out=[],idx=new Map();
  for(const x of rows){const k=x.section+'|'+x.reference;let p=idx.get(k);
   if(!p){p={key:k,section:x.section,reference:x.reference,kind:x.ref_kind,rows:[],markerNames:new Set(),refMarkers:{},members:x.reference_members,
     sectionText:{'single-single':'Single-Single','single-mixed':'Single-Mixed','mixed-mixed':'Mixed-Mixed'}[x.section]||'',
     title:x.ref_kind==='group'?T('pcRefGroup'):fmt('pcRefProfile',tab==='control'?'Custom Control':'Lab Reference')};idx.set(k,p);out.push(p)}
   p.rows.push(x);for(const [m,v] of Object.entries(x.markers)){p.markerNames.add(m);if(v.reference.length||!(m in p.refMarkers))p.refMarkers[m]=v.reference.length?v.reference:(p.refMarkers[m]||null)}}
  for(const p of out){for(const n of p.markerNames)if(!p.refMarkers[n])p.refMarkers[n]=null}return out}
 const plotFiles=()=>{if(tab==='concordance'){message(T('pcPlotNotConcordance'));return null}
  const keys=[...checks.entries()].filter(([k,v])=>v.on);if(!keys.length){message(T('pcNoPlotSelected'));return null}
  const files=new Set(),missing=[];for(const [,v] of keys){if(v.files.length)v.files.forEach(f=>files.add(f));else missing.push(v.name)}
  if(missing.length)message(fmt('pcPlotNotInProject',missing.join(',')));return [...files]};
 // ID-X ProfileConcordancePrintDlg: "Select the tab(s) to print:" with a check box per tab (enabled when the tab has results), then Print / Cancel.
 const TABS=[['concordance','pcConcordance'],['samples','pcSamples'],['lab_reference','pcLabRef'],['control','pcControl']];
 const hasData=id=>{const r=results[id];return !!r&&(id==='concordance'?(r.groups||[]).length>0:(r.rows||[]).length>0)};
 const buildPrintPage=ids=>{const keep=tab;const root=document.createElement('div');root.className='pcdlg pcprint';
  ids.forEach((id,i)=>{tab=id;const sec=el('section',{class:'pcprintsec',style:i?'page-break-before:always':''});sec.append(el('h2',{text:T(TABS.find(x=>x[0]===id)[1])}));
   if(id==='concordance'){for(const kind of ['single','mixed']){const list=(results.concordance.groups||[]).filter(g=>g.kind===kind);if(!list.length)continue;
     sec.append(el('div',{class:'pctitle'},el('b',{text:T(kind==='single'?'pcSingleSource':'pcMixedSource')}),el('span',{text:'   '+T('pc100Conc')})));
     for(const g of list){const fs=el('fieldset',{class:'pcgroup'},el('legend',{text:g.name}));
      for(const m of g.members){const x=byId(m);fs.append(el('div',{class:'pcsample',text:x?fullLabel(x):m}))}
      const ms=Object.entries(g.markers);const t=el('table',{class:'g pcpanel'});t.append(el('tr',{},...ms.map(e=>el('th',{text:e[0]}))));t.append(el('tr',{},...ms.map(e=>el('td',{class:'pcal',text:e[1].join(', ')}))));fs.append(t);sec.append(fs)}}}
   else{sec.append(el('div',{class:'note',text:T('pcThreshold')+' '+threshold+'%'}));let cur=null;
    const secTitle=p=>id==='samples'?{'single-single':T('pcSingleSingle'),'single-mixed':T('pcSingleMixed'),'mixed-mixed':T('pcMixedMixed')}[p.section]:p.title;
    for(const p of pcPanels(results[id].rows)){const st=secTitle(p);if(st!==cur){cur=st;sec.append(el('div',{class:'pctitle'},el('b',{text:st})))}sec.append(panelEl(p))}}
   root.append(sec)});
  tab=keep;
  root.querySelectorAll('input').forEach(i=>{if(i.type==='checkbox'){i.closest('td')?.replaceChildren();return}i.replaceWith(el('span',{text:i.value}))});
  return root};
 const printDialog=()=>{const avail=TABS.filter(([id])=>hasData(id));if(!avail.length){message(T('pcNoDataPrint'));return}
  dialog(T('print'),b=>{b.append(el('div',{text:T('pcSelectTabs')}));for(const [id,key] of TABS){const c=el('input',{type:'checkbox','data-tab':id});c.disabled=!hasData(id);c.checked=hasData(id);
    b.append(el('label',{class:'chk',style:'display:block;margin:6px 0 6px 20px'},c,document.createTextNode(' '+T(key))))}},
   [[T('print'),b=>{const ids=[...b.querySelectorAll('input:checked')].map(i=>i.dataset.tab);if(!ids.length)return false;
     const root=buildPrintPage(ids);const w=window.open('','opentrace-pc-print','width=1000,height=760');if(!w){message(T('plotWindowBlocked'));return false}
     w.document.open();w.document.write('<!doctype html><html><head><meta charset="utf-8"><title>'+T('profileComparison').replace(/[<&]/g,'')+'</title><link rel="stylesheet" href="/idx/idx.css"><style>body{background:#fff;overflow:auto;padding:12px}.pcprint h2{font-size:14px}</style></head><body>'+root.outerHTML+'</body></html>');
     w.document.close();w.onload=()=>{try{w.focus();w.print()}catch(e){}};setTimeout(()=>{try{w.print()}catch(e){}},600);return true},true],[T('cancel'),()=>true]])};
 const d=dialog(T('profileComparison').replace(/\.+$/,''),()=>{},[
  [T('displayPlots'),()=>{const f=plotFiles();if(f&&f.length)ctx.plot(f);return false}],
  [T('exportBtn'),()=>{exportCsv();return false}],
  [T('print'),()=>{printDialog();return false}],
  [T('closeWin'),()=>true,true]]);
 const markerGrid=(names,alleles,bold,other)=>names.map(n=>{const v=alleles[n]||[];const td=el('td',{class:'pcal'});
  v.forEach((a,i)=>{if(i)td.append(', ');const blue=other(n,a);const sp=el('span',{text:a});sp.style.color=blue?'#0000ff':'#000';if(bold)sp.style.fontWeight='bold';td.append(sp)});return td});
 const cbox=(key,name,files)=>{const c=el('input',{type:'checkbox','data-key':key,title:T('pcSelectPlot')});const st=checks.get(key)||{on:false,files,name};st.files=files;st.name=name;checks.set(key,st);c.checked=st.on;
  c.addEventListener('change',()=>{st.on=c.checked});return c};
 const filesFor=(name,members,isProfile)=>isProfile?all.filter(s=>(s.profileId||'')===name).map(s=>s.file):filesOf(members||[]);
 const panelEl=p=>{const names=[...p.markerNames];const t=el('table',{class:'g pcpanel'});
  const hd=el('tr',{},el('th'),el('th'),el('th'),...names.map(n=>el('th',{text:n})));t.append(hd);
  const refTip=p.members&&p.members.length>1?p.members.map(m=>{const s=byId(m);return s?fullLabel(s):m}).join('\n'):'';
  const others=new Map();for(const x of p.rows){for(const n of names){const m=x.markers[n];(others.get(n)||others.set(n,new Set()).get(n));for(const a of (m?.comparison||[]))others.get(n).add(String(a).trim().toLowerCase())}}
  const refRow=el('tr',{class:'pcref'},el('td',{},cbox('r|'+tab+'|'+p.key,p.reference,filesFor(p.reference,p.members,p.kind==='profile'))),
   el('td',{class:'b',title:refTip,text:fmt('pcReferenceOf',p.reference)}),el('td',{class:'b',text:T('pcMatch')}),
   ...markerGrid(names,Object.fromEntries(names.map(n=>[n,p.refMarkers[n]||[]])),true,(n,a)=>others.get(n)?.has(String(a).trim().toLowerCase())));
  t.append(refRow);
  const refSet=n=>new Set((p.refMarkers[n]||[]).map(a=>String(a).trim().toLowerCase()));
  for(const x of p.rows){const tip=x.comparison_members&&x.comparison_members.length>1?x.comparison_members.map(m=>{const s=byId(m);return s?fullLabel(s):m}).join('\n'):'';
   t.append(el('tr',{},el('td',{},cbox('c|'+tab+'|'+p.key+'|'+x.comparison,x.comparison,filesFor(x.comparison,x.comparison_members,tab!=='samples'&&x.ref_kind==='group'))),
    el('td',{title:tip,text:x.comparison}),el('td',{class:'num',text:pcPct(x.percent)}),
    ...markerGrid(names,Object.fromEntries(names.map(n=>[n,x.markers[n]?.comparison||[]])),false,(n,a)=>refSet(n).has(String(a).trim().toLowerCase()))))}
  if(tab==='samples'){const cm=el('input',{type:'text',maxlength:'100',class:'pccm',value:comments[p.key]||''});cm.addEventListener('input',()=>{comments[p.key]=cm.value});
   const tr=el('tr',{},el('td',{text:T('pcComments')}),el('td',{colspan:String(names.length+2)},cm));t.append(tr)}
  return t};
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('pcdlg');
  const tabs=el('div',{class:'tabs dtabs'});
  for(const [id,key] of [['concordance','pcConcordance'],['samples','pcSamples'],['lab_reference','pcLabRef'],['control','pcControl']]){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key),'data-tab':id});t.addEventListener('mousedown',()=>{tab=id;draw();if(id==='concordance'&&!results.concordance)run('concordance')});tabs.append(t)}
  const page=el('div',{class:'tabpage dpage'});
  page.append(el('div',{class:'note',text:fmt('pcEligible',eligible.length,all.length)}));
  if(tab==='concordance'){const r=results.concordance;
   const groups=r?.groups||[];
   if(r&&!groups.length)page.append(el('p',{class:'pcnone',text:T('pcNoGroups')}));
   for(const kind of ['single','mixed']){const list=groups.filter(g=>g.kind===kind);if(!list.length)continue;
    page.append(el('div',{class:'pctitle'},el('b',{text:T(kind==='single'?'pcSingleSource':'pcMixedSource')}),el('span',{text:'   '+T('pc100Conc')})));
    for(const g of list){const open=openSamples.has(g.name);const fs=el('fieldset',{class:'pcgroup'},el('legend',{text:g.name}));
     const tw=el('span',{class:'twisty',text:open?'−':'+'});const hd=el('div',{class:'pcsamples'},tw,el('span',{text:' '+T('samples')}));
     hd.addEventListener('mousedown',()=>{open?openSamples.delete(g.name):openSamples.add(g.name);draw()});fs.append(hd);
     if(open)for(const m of g.members){const s=byId(m);fs.append(el('div',{class:'pcsample',text:s?fullLabel(s):m}))}
     const ms=Object.entries(g.markers);const t=el('table',{class:'g pcpanel'});t.append(el('tr',{},...ms.map(e=>el('th',{text:e[0]}))));
     t.append(el('tr',{},...ms.map(e=>el('td',{class:'pcal',text:e[1].join(', ')}))));fs.append(t);page.append(fs)}}}
  else{const thr=el('input',{type:'text',id:'pcThreshold',maxlength:'3',value:String(threshold),class:'num'});
   thr.addEventListener('input',()=>{thr.value=thr.value.replace(/\D/g,'').slice(0,3);if(Number(thr.value)>100)thr.value=thr.value.slice(0,-1);threshold=thr.value===''?NaN:Number(thr.value)});
   const ig=el('input',{type:'text',id:'pcIgnore',value:ignore,placeholder:'F, A'});ig.addEventListener('change',()=>{ignore=ig.value;write('opentrace.pcIgnore',ignore)});
   const go=el('button',{class:'btn',type:'button',text:T('pcCompare'),id:'pcCompareBtn'});go.addEventListener('click',()=>run(tab));
   page.append(el('div',{class:'cap',text:T('pcThresholdFull')}),el('div',{class:'pcbar'},thr,go,el('label',{text:T('pcIgnoreLabels')}),ig));
   if(tab==='lab_reference'){const box=el('div',{class:'pcsel'});for(const s of eligible){const cb=el('input',{type:'checkbox','data-id':s.id});cb.checked=selected.has(s.id);cb.addEventListener('change',()=>{cb.checked?selected.add(s.id):selected.delete(s.id)});box.append(el('label',{class:'chk'},cb,document.createTextNode(' '+labelOf(s))))}
    page.append(el('div',{class:'cap pcnote',text:T('pcLabNote')}),box)}
   if(tab==='control')page.append(el('div',{class:'cap',text:fmt('pcControlNote',customControls().length)}));
   const r=results[tab];
   if(r&&r.rows&&r.rows.length){const panels=pcPanels(r.rows);let cur=null;
    const secTitle=p=>tab==='samples'?{'single-single':T('pcSingleSingle'),'single-mixed':T('pcSingleMixed'),'mixed-mixed':T('pcMixedMixed')}[p.section]:p.title;
    for(const p of panels){const st=secTitle(p);if(st!==cur){cur=st;page.append(el('div',{class:'pctitle'},el('b',{text:st})))}page.append(panelEl(p))}}}
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
