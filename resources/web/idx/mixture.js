// Mixture Analysis tool (Sample Selection -> Sample Segregation -> Results Viewer) and the
// Mixture Analysis Manager (methods, population databases). Engine: /api/mixture (mixture.py).
(function(){
const KEY='opentrace.mixture.v1';
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
let ctx=null; // {post, samples(), plot(files), results(), saveResults(r)}
let FACTORY=null;
const read=()=>{try{const v=JSON.parse(localStorage.getItem(KEY)||'null');if(v&&typeof v==='object')return v}catch(e){}return {methods:[],popdbs:[]}};
const write=v=>{try{localStorage.setItem(KEY,JSON.stringify(v));return true}catch(e){message(T('storageFull'));return false}};
async function factory(){if(!FACTORY)FACTORY=(await ctx.post('/api/mixture',{action:'factory'})).method;return clone(FACTORY)}
async function methods(){const db=read();const f=await factory();const list=db.methods.filter(m=>m.name!==f.name);return [f,...list]}
const getMethod=async name=>(await methods()).find(m=>m.name===name)||await factory();
const popdbs=()=>read().popdbs||[];
// Known genotypes imported into the Mixture Analysis tool (plus Lab References of the Profile Manager).
const KKEY='opentrace.knownGenotypes.v1';
const importedKnowns=()=>{try{const v=JSON.parse(localStorage.getItem(KKEY)||'[]');return Array.isArray(v)?v:[]}catch(e){return []}};
const saveKnowns=l=>{try{localStorage.setItem(KKEY,JSON.stringify(l));return true}catch(e){message(T('storageFull'));return false}};
const allKnowns=()=>{const imp=importedKnowns().map(k=>({...k,imported:true}));const ids=new Set(imp.map(k=>k.id));return [...imp,...ctx.knowns().filter(p=>!ids.has(p.id))]};
async function importKnown(){const f=await pickFile('.txt,.csv');if(!f)return null;
 try{const k=await ctx.post('/api/mixture',{action:'parse_known',text:f.text,filename:f.name});const list=importedKnowns();const i=list.findIndex(x=>x.id.toLowerCase()===k.id.toLowerCase());
  let saved=k;if(i>=0){saved=await ctx.post('/api/mixture',{action:'merge_known',existing:list[i],new:k});list[i]=saved}else list.push(k);
  if(!saveKnowns(list))return null;return saved}catch(e){message(e.message);return null}}
function knownDialog(id,onChange){const show=()=>allKnowns().find(k=>k.id===id);
 const d=dialog(T('maKnownGenotypes'),()=>{},[[T('ok'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('knowndlg');const k=show();
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},el('th',{text:T('colMarker')}),el('th',{text:T('maAlleles')}))));const tb=el('tbody');
  for(const [m,a] of Object.entries(k?.markers||{}))tb.append(el('tr',{},el('td',{text:m}),el('td',{text:a.join(', ')})));t.append(tb);
  const del=el('button',{class:'btn',type:'button',text:T('maDeleteKnown'),id:'maDelKnown'});del.disabled=!k?.imported;
  del.addEventListener('click',()=>{dialog(T('alert'),bb=>bb.append(el('p',{style:'max-width:420px',text:fmt('maDeleteKnownWarn',id)})),[[T('ok'),()=>{saveKnowns(importedKnowns().filter(x=>x.id!==id));onChange(null);d.close();return true},true],[T('cancel'),()=>true]])});
  const imp=el('button',{class:'btn',type:'button',text:T('maImportKnown'),id:'maImportKnown2'});imp.addEventListener('click',async()=>{const k2=await importKnown();if(k2){id=k2.id;onChange(id);draw()}});
  b.append(el('div',{class:'mabar'},el('b',{text:`${T('maKnown')}: ${id}`}),k?.imported?el('span',{class:'note',text:T('maKnownImported')}):el('span',{class:'note',text:T('maKnownProfile')})),el('div',{class:'grid magrid'},t),el('div',{class:'btnrow'},imp,del))};
 draw()}
const fShow=a=>/^F[123]$/.test(a)?'F':a;
const gt=g=>g.map(fShow).join(', ');
const q=v=>v==null?'NA':v;
const num=(v,d=3)=>v==null?'N/A':Number(v).toFixed(d);
function download(name,text,type){const url=URL.createObjectURL(new Blob([text],{type:type||'text/csv'}));const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
const csv=rows=>rows.map(r=>r.map(x=>{const s=x==null?'':String(x);return /[",\r\n]/.test(s)?'"'+s.replace(/"/g,'""')+'"':s}).join(',')).join('\r\n')+'\r\n';
function pickFile(accept){return new Promise(res=>{const i=el('input',{type:'file',accept,style:'display:none'});i.addEventListener('change',async()=>{const f=i.files[0];i.remove();res(f?{name:f.name,text:await f.text()}:null)});document.body.append(i);i.click()})}

// ---------- Mixture Analysis Manager ----------
function methodEditor(src,onSave){const m=clone(src);
 const d=dialog(T('maMethodEditor'),()=>{},[[T('ok'),async()=>{try{const v=(await ctx.post('/api/mixture',{action:'validate',method:m})).method;onSave(v);return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('mamdlg');
  const name=el('input',{type:'text',id:'maName',value:m.name});name.addEventListener('input',()=>m.name=name.value);
  const desc=el('input',{type:'text',value:m.description||''});desc.addEventListener('input',()=>m.description=desc.value);
  const mit=el('input',{type:'number',id:'maMit',class:'num',value:String(m.mit)});mit.addEventListener('change',()=>m.mit=Number(mit.value));
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('maMinH'),T('maMaxH'),T('maPhr')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  m.phr.forEach((r,i)=>{const cells=r.map((v,j)=>{const x=el('input',{type:'number',class:'num',value:String(v),step:j===2?'0.01':'1','data-r':String(i),'data-c':String(j)});x.addEventListener('change',()=>{r[j]=Number(x.value)});return el('td',{},x)});tb.append(el('tr',{},...cells))});
  t.append(tb);
  const add=el('button',{class:'btn',type:'button',text:T('maNewRow'),id:'maAddRow'});add.addEventListener('click',()=>{const last=m.phr[m.phr.length-1];const lo=last?last[1]+1:m.mit;if(lo>99999)return message(T('maRangeMax'));m.phr.push([lo,Math.min(99999,lo+999),0.5]);draw()});
  const del=el('button',{class:'btn',type:'button',text:T('maDeleteRow')});del.addEventListener('click',()=>{if(m.phr.length>1){m.phr.pop();draw()}});
  b.append(el('div',{class:'field'},el('label',{text:T('amName')}),name),el('div',{class:'field'},el('label',{text:T('amDescription')}),desc),
   el('fieldset',{},el('legend',{text:T('maPhrSettings')}),el('div',{class:'grid'},t),el('div',{class:'btnrow'},add,del),el('p',{class:'note',text:T('maPhrNote')})),
   el('fieldset',{},el('legend',{text:T('maMitTitle')}),el('div',{class:'field'},el('label',{text:T('maMit')}),mit),el('p',{class:'note',text:T('maMitNote')})))};
 draw()}
async function manager(){let tab='methods',sel=null;
 const d=dialog(T('mixtureManager').replace(/\.+$/,''),()=>{},[[T('mgrDone'),()=>true,true]]);
 const draw=async()=>{const b=d.body;b.replaceChildren();b.classList.add('mamgr');
  const tabs=el('div',{class:'tabs dtabs'});for(const [id,key] of [['methods','maMethods'],['popdbs','maPopDbs']]){const x=el('div',{class:'tab'+(tab===id?' on':''),text:T(key),'data-tab':id});x.addEventListener('mousedown',()=>{tab=id;sel=null;draw()});tabs.append(x)}
  const t=el('table',{class:'g'});const tb=el('tbody');const btn=(text,fn,id)=>{const x=el('button',{class:'btn',type:'button',text,id});x.addEventListener('click',fn);return x};
  const row=(cells,key)=>{const tr=el('tr',{class:key===sel?'sel':''},...cells.map(c=>el('td',{text:c})));tr.addEventListener('mousedown',()=>{sel=key;tb.querySelectorAll('tr').forEach(x=>x.classList.toggle('sel',x===tr))});tb.append(tr);return tr};
  let buttons;
  if(tab==='methods'){t.append(el('thead',{},el('tr',{},...[T('colName'),T('colLastSaved'),T('amDescription').replace(':','')].map(h=>el('th',{text:h})))));
   for(const m of await methods()){const tr=row([m.name,m.saved?new Date(m.saved).toLocaleString():'—',m.description||''],m.name);tr.addEventListener('dblclick',()=>openM(m.name))}
   const save=(m,prev)=>{const db=read();db.methods=db.methods.filter(x=>x.name!==prev&&x.name!==m.name);if(m.name===FACTORY?.name)return message(T('maFactoryName'));db.methods.push({...m,saved:new Date().toISOString()});write(db);sel=m.name;draw()};
   const openM=async name=>{const m=await getMethod(name);if(name===FACTORY?.name)return message(T('maFactoryReadOnly'));methodEditor(m,v=>save(v,name))};
   buttons=[btn(T('mgrNew'),async()=>{const f=await factory();let n=1;while((await methods()).some(m=>m.name===`${T('maUntitled')} ${n}`))n++;f.name=`${T('maUntitled')} ${n}`;f.description='';methodEditor(f,v=>save(v,null))},'maNew'),
    btn(T('mgrOpen'),()=>{if(!sel)return message(T('selectRow'));openM(sel)}),
    btn(T('mgrSaveAs'),async()=>{if(!sel)return message(T('selectRow'));const m=await getMethod(sel);m.name=m.name+' copy';methodEditor(m,v=>save(v,null))}),
    btn(T('mgrImport'),async()=>{const f=await pickFile('.json');if(!f)return;try{const m=JSON.parse(f.text);const v=(await ctx.post('/api/mixture',{action:'validate',method:m})).method;save(v,null)}catch(e){message(e.message)}}),
    btn(T('mgrExport'),async()=>{if(!sel)return message(T('selectRow'));const m=await getMethod(sel);download(m.name+'.json',JSON.stringify(m,null,1),'application/json')}),
    btn(T('mgrDelete'),()=>{if(!sel||sel===FACTORY?.name)return;const db=read();db.methods=db.methods.filter(x=>x.name!==sel);write(db);sel=null;draw()})]}
  else{t.append(el('thead',{},el('tr',{},...[T('maDatabase'),T('maSource'),T('maPopulations'),T('colLastSaved')].map(h=>el('th',{text:h})))));
   for(const p of popdbs())row([p.data.database,p.data.source||'',p.data.populations.map(x=>x.name).join(', '),new Date(p.saved).toLocaleString()],p.data.database);
   buttons=[btn(T('mgrImport'),async()=>{const f=await pickFile('.csv,.txt');if(!f)return;try{const data=await ctx.post('/api/mixture',{action:'parse_popdb',text:f.text});const db=read();db.popdbs=(db.popdbs||[]).filter(x=>x.data.database!==data.database);db.popdbs.push({data,saved:new Date().toISOString(),file:f.name});if(write(db)){sel=data.database;draw()}}catch(e){message(e.message)}},'maImportDb'),
    btn(T('maView'),()=>{const p=popdbs().find(x=>x.data.database===sel);if(!p)return message(T('selectRow'));viewDb(p.data)}),
    btn(T('mgrDelete'),()=>{if(!sel)return;const db=read();db.popdbs=db.popdbs.filter(x=>x.data.database!==sel);write(db);sel=null;draw()})]}
  t.append(tb);b.append(tabs,el('div',{class:'grid mgrgrid'},t),el('div',{class:'btnrow'},...buttons));
  if(tab==='popdbs')b.append(el('p',{class:'note',text:T('maPopNote')}))};
 draw()}
function viewDb(data){let pi=0;const d=dialog(data.database,()=>{},[[T('closeWin'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();const sel=el('select');data.populations.forEach((p,i)=>sel.append(new Option(`${p.name} (θ=${p.theta})`,String(i))));sel.value=String(pi);sel.addEventListener('change',()=>{pi=+sel.value;draw()});
  const pop=data.populations[pi];const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('colMarker'),'N',T('colAllele'),T('maFrequency')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const [m,v] of Object.entries(pop.markers))for(const [a,f] of Object.entries(v.freq))tb.append(el('tr',{},el('td',{text:m}),el('td',{class:'num',text:String(v.n)}),el('td',{text:a}),el('td',{class:'num',text:f==null?'*':String(f)})));
  t.append(tb);b.append(el('div',{class:'field'},el('label',{text:T('maPopulation')}),sel),el('div',{class:'grid mgrgrid'},t))};draw()}

// ---------- tool ----------
async function open(){
 if(!ctx)return;const all=ctx.samples();const meths=await methods();let saved=ctx.results()||{};
 const eligible=all.filter(s=>s.eligibleBase&&!s.ol);const ol=all.filter(s=>s.eligibleBase&&s.ol);
 if(!eligible.length&&!ol.length)return message(T('maNoSamples'));
 const st={page:'select',methodOf:{},include:new Set(eligible.map(s=>s.key)),seg:null,tab:2,known:{},current:null,results:saved};
 for(const s of eligible)st.methodOf[s.key]=saved[s.key]?.method||meths[0].name;
 const d=dialog(T('mixture').replace(/\.+$/,''),()=>{},[]);d.body.classList.add('madlg');
 const foot=d.root.querySelector('.foot');
 const setButtons=list=>{foot.replaceChildren();for(const [label,fn,id,dis] of list){const b=el('button',{class:'btn',type:'button',text:label,id});b.disabled=!!dis;b.addEventListener('click',fn);foot.append(b)}};
 const finish=()=>{ctx.saveResults(st.results);d.close()};
 const draw=()=>{const b=d.body;b.replaceChildren();
  if(st.page==='select'){b.append(el('h3',{text:T('maSelectTitle')}),el('p',{class:'note',text:T('maSelectNote')}));
   const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...['',T('sumRunFolder'),T('colSampleFile'),T('colSampleName'),T('maMethod')].map(h=>el('th',{text:h})))));const tb=el('tbody');
   for(const s of eligible){const cb=el('input',{type:'checkbox','data-file':s.key});cb.checked=st.include.has(s.key);cb.addEventListener('change',()=>{cb.checked?st.include.add(s.key):st.include.delete(s.key)});
    const ms=el('select');for(const m of meths)ms.append(new Option(m.name,m.name));ms.value=st.methodOf[s.key];ms.addEventListener('change',()=>{st.methodOf[s.key]=ms.value});
    tb.append(el('tr',{},el('td',{class:'ic'},cb),el('td',{text:s.runFolder}),el('td',{text:s.file}),el('td',{text:s.name}),el('td',{},ms)))}
   t.append(tb);b.append(el('div',{class:'grid magrid'},t));
   setButtons([[T('maDetails'),()=>message(ol.length?T('maOlList')+'\n'+ol.map(s=>s.name).join('\n'):T('maOlNone'))],[T('mixtureManager').replace(/\.+$/,''),()=>manager()],
    [T('cancel'),()=>d.close()],[T('maNext'),()=>segregate(),'maNext']])}
  else if(st.page==='segregate'){const groups={1:[],2:[],3:[]};for(const r of st.seg)if(r.contributors)groups[r.contributors].push(r);
   const tabs=el('div',{class:'tabs dtabs'});for(const k of [1,2,3]){const x=el('div',{class:'tab'+(st.tab===k?' on':''),text:`${T('maTab'+k)} (${groups[k].length})`,'data-tab':String(k)});x.addEventListener('mousedown',()=>{st.tab=k;draw()});tabs.append(x)}
   const t=el('table',{class:'g'});const heads=[T('sumRunFolder'),T('colSampleFile'),T('colSampleName'),T('maMethod'),T('maReason')];if(st.tab===2)heads.unshift(T('maExtraction'),T('maReview'));if(st.tab===2)heads.push(T('maKnown'));
   t.append(el('thead',{},el('tr',{},...heads.map(h=>el('th',{text:h})))));const tb=el('tbody');
   for(const r of groups[st.tab]){const s=all.find(x=>x.key===r.id);const res=st.results[r.id];const cells=[];
    if(st.tab===2)cells.push(el('td',{class:'ic',html:res?.extraction?.status==='extracted'?'':pqv('check')}),el('td',{text:res?.review?'TRUE':'FALSE'}));
    cells.push(el('td',{text:s.runFolder}),el('td',{text:s.file}),el('td',{text:s.name}),el('td',{text:st.methodOf[r.id]}),el('td',{text:reasonText(r)}));
    if(st.tab===2){const ks=el('select',{'data-known':r.id});ks.append(new Option(T('maNone'),''));for(const p of allKnowns())ks.append(new Option(p.id,p.id));ks.append(new Option(T('maImportOption'),'__import__'));
     ks.value=st.known[r.id]||'';ks.addEventListener('change',async()=>{if(ks.value==='__import__'){const k=await importKnown();if(k){st.known[r.id]=k.id;knownDialog(k.id,v=>{st.known[r.id]=v||'';draw()})}draw();return}st.known[r.id]=ks.value});
     const td=el('td',{},ks);td.addEventListener('dblclick',()=>{if(st.known[r.id])knownDialog(st.known[r.id],v=>{const old=st.known[r.id];if(!v){for(const sid of Object.keys(st.known))if(st.known[sid]===old)st.known[sid]=''}else st.known[r.id]=v;draw()})});cells.push(td)}
    tb.append(el('tr',{},...cells))}
   t.append(tb);b.append(el('h3',{text:T('maSegTitle')}),tabs,el('div',{class:'grid magrid'},t),el('p',{class:'note',text:T('maSegNote'+st.tab)}));
   const exp=()=>download('Sample Segregation.csv',csv([[T('maContributors'),T('colSampleFile'),T('colSampleName'),T('maMethod'),T('maReason')],...st.seg.filter(r=>r.contributors).map(r=>[r.contributors===3?'3+':r.contributors,r.id,all.find(x=>x.key===r.id).name,st.methodOf[r.id],reasonText(r)])]));
   setButtons([[T('exportBtn'),exp],[T('maBack'),()=>{st.page='select';draw()}],[T('maFinish'),finish],[T('maNext'),()=>extractAll(groups),'maNext2']])}
  else viewer(b)};
 const reasonText=r=>!r.reason?'':r.reason.startsWith('phr_fail:')?fmt('maReasonPhr',r.reason.slice(9)):T('maReason_'+r.reason);
 async function segregate(){const list=eligible.filter(s=>st.include.has(s.key));if(!list.length)return message(T('selectSamplesFirst'));
  try{const out=[];for(const s of list){const m=await getMethod(st.methodOf[s.key]);const r=(await ctx.post('/api/mixture',{action:'segregate',method:m,samples:[{id:s.key,markers:s.markers}]})).samples[0];out.push(r)}
   st.seg=out;for(const r of out){const prev=st.results[r.id];const mit=(await getMethod(st.methodOf[r.id])).mit;if(!prev||prev.method!==st.methodOf[r.id]||prev.contributors!==r.contributors)st.results[r.id]={method:st.methodOf[r.id],mit,contributors:r.contributors,adbi:r.adbi_markers,reason:r.reason}}
   st.tab=[2,1,3].find(k=>out.some(r=>r.contributors===k))||1;st.page='segregate';draw()}catch(e){message(e.message)}}
 async function extractAll(groups){
  for(const r of groups[2]){const res=st.results[r.id];const kn=st.known[r.id]?allKnowns().find(p=>p.id===st.known[r.id]):null;
   if(res.extraction&&res.knownId===(kn?.id||''))continue;
   const s=all.find(x=>x.key===r.id);try{res.extraction=await ctx.post('/api/mixture',{action:'extract',method:await getMethod(res.method),sample:{id:s.key,markers:s.markers},known:kn?kn.markers:null});res.knownId=kn?.id||'';res.review=false}catch(e){message(e.message);return}}
  for(const k of [1,3])for(const r of groups[k]){const res=st.results[r.id];res.options=res.options||{}}
  st.page='viewer';const order=[...groups[2],...groups[1],...groups[3]].map(r=>r.id);st.order=order;st.current=order[0];draw()}
 // ---------- results viewer ----------
 const sOf=id=>st.virtual?.[id]||all.find(x=>x.key===id);
 function viewer(b){const id=st.current;const s=sOf(id);const res=st.results[id];
  const nav=el('div',{class:'mahead'});const sel=el('select',{id:'maSample'});for(const f of st.order)sel.append(new Option(sOf(f).name,f));sel.value=id;sel.addEventListener('change',()=>{st.current=sel.value;st.vtab=null;draw()});
  const i=st.order.indexOf(id);const prev=el('button',{class:'btn',type:'button',text:'<< '+T('maPrevSample')});prev.disabled=i<=0;prev.addEventListener('click',()=>{st.current=st.order[i-1];st.vtab=null;draw()});
  const next=el('button',{class:'btn',type:'button',text:T('maNextSample')+' >>',id:'maNextSample'});next.disabled=i>=st.order.length-1;next.addEventListener('click',()=>{st.current=st.order[i+1];st.vtab=null;draw()});
  nav.append(el('span',{text:`${T('colSampleFile')}: ${s.file}`}),el('span',{text:`${T('sumRunFolder')}: ${s.runFolder}`}),el('span',{text:`${T('colPanel')}: ${s.panel}`}),el('span',{text:`${T('maContributors')}: ${res.contributors===3?'3+':res.contributors}`}),el('span',{},el('label',{text:T('colSampleName')+': '}),sel),prev,next);
  b.append(nav);
  const tabs=[['results',T(res.contributors===1?'ma1Results':res.contributors===2?'maMixResults':'ma3Results')]];
  if(res.stats?.rmp)tabs.push(['rmp',T('maRmpTab')]);if(res.stats?.cpi)tabs.push(['cpi',T('maCpiTab')]);if(res.stats&&res.contributors<=2)tabs.push(['lr',T('maLrTab')]);
  st.vtab=st.vtab&&tabs.some(t=>t[0]===st.vtab)?st.vtab:'results';
  const tb=el('div',{class:'tabs dtabs'});for(const [k,label] of tabs){const x=el('div',{class:'tab'+(st.vtab===k?' on':''),text:label,'data-tab':k});x.addEventListener('mousedown',()=>{st.vtab=k;draw()});tb.append(x)}
  const page=el('div',{class:'tabpage dpage mapage'});b.append(tb,page);
  if(st.vtab==='results'){if(res.merged)mergedView(page,s,res);else if(res.contributors===1)oneContributor(page,s,res);else if(res.contributors===2)twoContributors(page,s,res);else threePlus(page,s,res)}
  else if(st.vtab==='rmp')rmpView(page,res);else if(st.vtab==='lr')lrView(page,s,res);else cpiView(page,res);
  const notes=()=>dialog(T('maNotes'),bb=>{const ta=el('textarea',{rows:'6',cols:'60',id:'maNoteText'});ta.value=res.notes||'';bb.append(ta)},[[T('ok'),bb=>{res.notes=bb.querySelector('#maNoteText').value}],[T('cancel'),()=>true]]);
  setButtons([[T('maNotes'),notes],[T('maCalcStats'),()=>calcStats(s,res),'maCalcStats'],[T('exportBtn'),()=>exportResults(s,res)],[T('maParams'),()=>params(res)],[T('maSamplesPlot'),()=>ctx.plot(res.merged?res.merged.markers.flatMap(m=>m.variants.map(v=>v.source)).filter((v,i,a)=>a.indexOf(v)===i):[s.key])],[T('maMergedBtn'),()=>mergeGroups(),'maMergedBtn',!mergeCandidates().length],
   [T('maBack'),()=>{st.page='segregate';draw()}],[T('maFinish'),finish,'maFinish']])}
 // Alleles at or above the MIT of the sample's mixture analysis method (Y markers excluded).
 const alleles=(s,res)=>Object.fromEntries(s.markers.filter(m=>!m.y).map(m=>[m.name,m.alleles.filter(a=>a.height>=(res?.mit||0)).map(a=>a.label)]));
 function oneContributor(page,s,res){res.options=res.options||{};const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('colMarker'),T('maAlleles'),T('maOption')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const [m,al] of Object.entries(alleles(s,res))){if(!al.length)continue;const opts=al.length===1?[['alleles','maUseAlleles'],['2p','maUse2p'],['inconclusive','maInconclusive']]:al.length===2?[['alleles','maUseAlleles'],['inconclusive','maInconclusive']]:[['min','maMinGenotype'],['inconclusive','maInconclusive']];
   const sel=el('select');for(const [v,k] of opts)sel.append(new Option(T(k),v));sel.value=res.options[m]||opts[0][0];sel.addEventListener('change',()=>{res.options[m]=sel.value;res.stats=null});
   tb.append(el('tr',{},el('td',{text:m}),el('td',{text:al.join(', ')}),el('td',{},sel)))}
  t.append(tb);page.append(el('div',{class:'grid magrid'},t))}
 function threePlus(page,s,res){const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('colMarker'),T('maAlleles')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const [m,al] of Object.entries(alleles(s,res)))if(al.length)tb.append(el('tr',{},el('td',{text:m}),el('td',{text:al.join(', ')})));t.append(tb);page.append(el('p',{class:'note',text:T('maSegNote3')}),el('div',{class:'grid magrid'},t))}
 function twoContributors(page,s,res){const x=res.extraction;
  if(!x||x.status!=='extracted'){page.append(el('p',{text:x?.message||T('maNotExtracted')}));return}
  st.filter=st.filter||{marker:'All',iq:'all',show:'both',known:false};const f=st.filter;const kc=x.known_contributor;
  const hdr=el('div',{class:'mabar'},el('span',{text:`${T('maAvgMx')}: ${num(x.average_mx,4)}`,id:'maAvgMx'}),el('span',{text:`${T('maResidualThr')}: ${x.residual_threshold}`}),
   el('span',{text:`${T('maKnown')}: ${res.knownId||T('maNone')}`}),el('span',{text:`${T('maKnownMatch')}: ${x.known_match||'N/A'}`}));
  const mk=el('select',{id:'maFilterMarker'});mk.append(new Option(T('maAll'),'All'));for(const m of [...new Set(x.rows.map(r=>r.marker))])mk.append(new Option(m,m));mk.value=f.marker;mk.addEventListener('change',()=>{f.marker=mk.value;draw()});
  const iq=el('select',{id:'maFilterIq'});for(const [v,k] of [['all','maShowAll'],['pass','maShowPass'],['check','maShowCheck'],['low','maShowLow']])iq.append(new Option(T(k),v));iq.value=f.iq;iq.addEventListener('change',()=>{f.iq=iq.value;draw()});
  const show=el('select',{id:'maShow'});for(const [v,k] of [['both','maShowBoth'],['selected','maShowSelected'],['unselected','maShowUnselected']])show.append(new Option(T(k),v));show.value=f.show;show.addEventListener('change',()=>{f.show=show.value;draw()});
  const rev=el('input',{type:'checkbox',id:'maReview'});rev.checked=!!res.review;rev.addEventListener('change',()=>{res.review=rev.checked;draw()});
  page.append(hdr,el('div',{class:'mabar'},el('label',{text:T('maFilterMarker')}),mk,el('label',{text:T('maFilterIq')}),iq,el('label',{text:T('maTables')}),show,el('label',{class:'chk'},rev,document.createTextNode(' '+T('maReviewComplete')))));
  const selRows=new Set();
  const table=(selected,title)=>{const rows=x.rows.filter(r=>r.selected===selected&&(f.marker==='All'||r.marker===f.marker)&&(selected||f.iq==='all'||r.iq===f.iq)&&(selected||!f.known||!r.known?.length||(kc?r.known_match?.includes(kc.toUpperCase()):!!r.known_match)));
   const t=el('table',{class:'g matab'});const heads=[T('colMarker'),'C1 (Major)','C2 (Minor)',T('maInc')+' C1',T('maInc')+' C2','ADBI','Mx',T('maResidual'),'PHR1','PHR2',T('maResStatus'),T('maPhrStatus'),'IQ'];if(x.known_match)heads.push(T('maKnownGt'));
   t.append(el('thead',{},el('tr',{},...heads.map(h=>el('th',{text:h})))));const tb=el('tbody');
   for(const r of rows){const ci=r.inconclusive;const inc=k=>{const c=el('input',{type:'checkbox'});c.checked=!!ci[k];c.disabled=!selected;c.addEventListener('change',()=>{ci[k]=c.checked;res.stats=null});c.addEventListener('mousedown',e=>e.stopPropagation());return el('td',{class:'ic'},c)};
    // ID-X: matched contributor bright green, unknown contributor bright yellow (pale when undetermined)
    const kcls=c=>{if(!x.known_match||!r.known?.length)return '';const pale=kc?'':' pale';const k=kc||(r.known_lean||'C1').toLowerCase();if(!r.known_match?.includes(k.toUpperCase()))return '';return (c===k?'kmatch':'kunknown')+pale};
    const tr=el('tr',{class:(r.known_match?'known ':'')},el('td',{text:r.marker}),el('td',{text:gt(r.c1),class:kcls('c1')}),el('td',{text:gt(r.c2),class:kcls('c2')}),inc('c1'),inc('c2'),
     el('td',{class:'ic',html:pqv(r.adbi)}),el('td',{class:'num',text:r.mx?num(r.mx):'N/A'}),el('td',{class:'num',text:num(r.residual,4)}),el('td',{class:'num',text:num(r.phr1)}),el('td',{class:'num',text:num(r.phr2)}),
     el('td',{class:'ic',html:pqv(r.residual_status)}),el('td',{class:'ic',html:pqv(r.phr_status)}),el('td',{class:'ic',html:pqv(r.iq)}));
    if(x.known_match)tr.append(el('td',{text:r.known?.length?r.known.join(', '):''}));
    tr.addEventListener('mousedown',e=>{if(!(e.ctrlKey||e.metaKey))selRows.clear(),tb.querySelectorAll('tr').forEach(z=>z.classList.remove('sel'));selRows.add(r);tr.classList.add('sel')});tb.append(tr)}
   t.append(tb);return el('div',{},el('div',{class:'cap',text:`${title} (${rows.length})`}),el('div',{class:'grid magrid'+(selected?' sel':'')},t))};
  const move=to=>{if(!selRows.size)return message(T('selectRow'));for(const r of selRows){r.selected=to;if(!to)r.inconclusive={c1:false,c2:false}}x.missing_markers=[...new Set(x.rows.map(r=>r.marker))].filter(m=>!x.rows.some(r=>r.marker===m&&r.selected)).sort();res.stats=null;draw()};
  const bar=el('div',{class:'btnrow'});const bt=(text,fn,id)=>{const z=el('button',{class:'btn',type:'button',text,id});z.addEventListener('click',fn);return z};
  const missing=()=>[...new Set([...x.missing_markers,...(x.known_missing_markers||[])])].sort();
  bar.append(bt(T('maUnselect'),()=>move(false),'maUnselect'),bt(T('maSelect'),()=>move(true),'maSelectBtn'),bt(T('maMissing'),()=>message(missing().length?T('maMissingList')+'\n'+missing().join(', '):T('maMissingNone')),'maMissingBtn'));
  if(x.known_match){const fk=bt(f.known?T('maShowAllCombos'):T('maFilterKnown'),()=>{f.known=!f.known;draw()},'maFilterKnown');bar.append(fk)}
  if(f.show!=='unselected')page.append(table(true,T('maSelectedTable')));
  page.append(bar);
  if(f.show!=='selected')page.append(table(false,T('maUnselectedTable')))}
 function contributorProfile(res,key){const prof={};for(const r of res.extraction.rows){if(!r.selected)continue;const p=prof[r.marker]=prof[r.marker]||{genotypes:[],inconclusive:false};const g=r[key].map(a=>/^F[123]$/.test(a)?'F':a);if(!p.genotypes.some(x=>[...x].sort().join()===[...g].sort().join()))p.genotypes.push(g);if(r.inconclusive[key])p.inconclusive=true}return prof}
 async function calcStats(s,res){const dbs=popdbs();if(!dbs.length)return message(T('maNoPopDb'));let di=0;const pops=new Set(dbs[0].data.populations.map(p=>p.name));
  dialog(T('maPopSelect'),b=>{const sel=el('select',{id:'maDb'});dbs.forEach((p,i)=>sel.append(new Option(p.data.database,String(i))));const box=el('div',{class:'pcsel',id:'maPops'});
   const fill=()=>{box.replaceChildren();pops.clear();for(const p of dbs[di].data.populations){pops.add(p.name);const c=el('input',{type:'checkbox'});c.checked=true;c.addEventListener('change',()=>{c.checked?pops.add(p.name):pops.delete(p.name)});box.append(el('label',{class:'chk'},c,document.createTextNode(` ${p.name} (θ=${p.theta})`)))}};
   sel.addEventListener('change',()=>{di=+sel.value;fill()});fill();b.append(el('div',{class:'field'},el('label',{text:T('maDatabase')}),sel),el('div',{class:'cap',text:T('maPopulations')}),box,el('p',{class:'note',text:T('maStatsNote')}))},
   [[T('ok'),async()=>{const db=dbs[di].data,names=[...pops];if(!names.length)return message(T('maPickPop')),false;res.stats={database:db.database,populations:names};res.lr=null;
     try{if(res.contributors===1)res.stats.rmp=await ctx.post('/api/mixture',{action:'statistics',kind:'single',population_db:db,populations:names,markers:alleles(s,res),options:res.options});
      if(res.contributors===2){const x=res.extraction;if(x?.status!=='extracted')return message(T('maNotExtracted')),false;
       res.stats.rmp=await ctx.post('/api/mixture',{action:'statistics',kind:'two',population_db:db,populations:names,c1:contributorProfile(res,'c1'),c2:contributorProfile(res,'c2'),known_contributor:x.known_contributor||null});
       const drop=[...new Set(x.rows.filter(r=>r.selected&&r.dropout).map(r=>r.marker))];
       res.stats.cpi=await ctx.post('/api/mixture',{action:'statistics',kind:'cpi',population_db:db,populations:names,markers:alleles(s,res),dropout_markers:drop})}
      if(res.contributors===3)res.stats.cpi=await ctx.post('/api/mixture',{action:'statistics',kind:'cpi',population_db:db,populations:names,markers:alleles(s,res)});
      st.vtab=res.stats.rmp?'rmp':'cpi';draw();return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]])}
 const big=v=>v==null?'N/A':v>=1e6?Number(v).toExponential(3):Math.round(v).toLocaleString();
 function rmpView(page,res){const r=res.stats.rmp;const two=r.kind==='two';const t=el('table',{class:'g'});const KN={known:true,profile_frequency:null,rmp:null,missing_markers:[],details:[]};
  if(two)r.results=r.results.map(p=>({...p,c1:p.c1||KN,c2:p.c2||KN}));const pf=c=>c.known?T('maKnownContrib'):c.profile_frequency.toExponential(4),rm=c=>c.known?'—':'1 / '+big(c.rmp);
  t.append(el('thead',{},el('tr',{},...[T('maPopulation'),'θ',...(two?['C1 '+T('maProfileFreq'),'C1 RMP','C2 '+T('maProfileFreq'),'C2 RMP']:[T('maProfileFreq'),'RMP']),T('maMissingPop')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const p of r.results){const cells=two?[pf(p.c1),rm(p.c1),pf(p.c2),rm(p.c2)]:[p.profile_frequency.toExponential(4),'1 / '+big(p.rmp)];
   tb.append(el('tr',{},el('td',{text:p.population}),el('td',{text:String(p.theta)}),...cells.map(c=>el('td',{class:'num',text:c})),el('td',{text:(two?[...new Set([...p.c1.missing_markers,...p.c2.missing_markers])]:p.missing_markers).join(', ')})))}
  t.append(tb);page.append(el('div',{class:'cap',text:`${T('maRmpSummary')} — ${r.database}`}),el('div',{class:'grid'},t));
  const d=el('table',{class:'g'});d.append(el('thead',{},el('tr',{},...[T('maPopulation'),T('colMarker'),...(two?['C1','C1 '+T('maCombFreq'),'C2','C2 '+T('maCombFreq')]:[T('maAlleles'),T('maGenoFreq')])].map(h=>el('th',{text:h})))));const db=el('tbody');
  for(const p of r.results){if(two){const ms=[...new Set([...p.c1.details.map(x=>x.marker),...p.c2.details.map(x=>x.marker)])];for(const m of ms){const a=p.c1.details.find(x=>x.marker===m),c=p.c2.details.find(x=>x.marker===m);
     db.append(el('tr',{},el('td',{text:p.population}),el('td',{text:m}),el('td',{text:a?.genotypes?a.genotypes.map(gt).join(' | '):a?.note||''}),el('td',{class:'num',text:a?a.frequency.toFixed(4):''}),el('td',{text:c?.genotypes?c.genotypes.map(gt).join(' | '):c?.note||''}),el('td',{class:'num',text:c?c.frequency.toFixed(4):''})))}}
   else for(const x of p.details)db.append(el('tr',{},el('td',{text:p.population}),el('td',{text:x.marker}),el('td',{text:x.genotypes?x.genotypes.map(gt).join(' | '):x.note}),el('td',{class:'num',text:x.frequency.toFixed(4)})))}
  d.append(db);page.append(el('div',{class:'cap',text:T('maRmpDetails')}),el('div',{class:'grid magrid'},d))}
 function cpiView(page,res){const r=res.stats.cpi;const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('maPopulation'),'CPI','1 in','CPE %',T('maMissingPop')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const p of r.results)tb.append(el('tr',{},el('td',{text:p.population}),el('td',{class:'num',text:p.cpi.toExponential(4)}),el('td',{class:'num',text:big(p.one_in)}),el('td',{class:'num',text:(p.cpe*100).toFixed(4)}),el('td',{text:p.missing_markers.join(', ')})));
  t.append(tb);const d=el('table',{class:'g'});d.append(el('thead',{},el('tr',{},...[T('maPopulation'),T('colMarker'),T('maAlleles'),T('maCombos'),T('maInclusion')].map(h=>el('th',{text:h})))));const db=el('tbody');
  for(const p of r.results)for(const x of p.details)db.append(el('tr',{},el('td',{text:p.population}),el('td',{text:x.marker}),el('td',{text:x.alleles.join(', ')}),el('td',{class:'num',text:x.combinations==null?'N/A':String(x.combinations)}),el('td',{class:'num',text:x.inclusion.toFixed(4)})));
  d.append(db);page.append(el('div',{class:'cap',text:`${T('maCpiSummary')} — ${r.database}`}),el('div',{class:'grid'},t),el('div',{class:'cap',text:T('maCpiDetails')}),el('div',{class:'grid magrid'},d))}
 // ---------- likelihood ratio ----------
 const hypText=(h,r1,r2)=>{const U=T('maUnknown'),UU=T('maTwoUnknown');const mix=(a,b)=>fmt('maMixtureOf',a,b);
  return {single:[`C1: ${r1}`,`C1: ${U}`],major:[`C1: ${r1}`,`C1: ${U}`],h1:[mix(r1,r2),mix(r1,U)],h2:[mix(r1,r2),mix(r2,U)],h3:[mix(r1,r2),UU],h4:[mix(r1,U),UU],h5:[mix(r2,U),UU]}[h]};
 function lrView(page,s,res){const L=res.lr=res.lr||{r1:res.knownId||'',r2:'',useC1:false,result:null,checked:null};const two=res.contributors===2;const knowns=allKnowns();
  const refSel=(key,id,other)=>{const x=el('select',{id});x.append(new Option(T('maNone'),''));for(const p of knowns)if(p.id!==L[other])x.append(new Option(p.id,p.id));x.value=L[key];x.addEventListener('change',()=>{L[key]=x.value;L.result=null;draw()});return x};
  const bar=el('div',{class:'mabar'},el('span',{text:'LR = Pr(E | Hp) / Pr(E | Hd)'}),el('label',{text:T('maRef1')}),refSel('r1','maRef1','r2'));
  if(two){const c=el('input',{type:'checkbox',id:'maUseC1'});c.checked=L.useC1;c.addEventListener('change',()=>{L.useC1=c.checked;if(c.checked)L.r2='';L.result=null;draw()});
   const r2=refSel('r2','maRef2','r1');r2.disabled=L.useC1;bar.append(el('label',{text:T('maRef2')}),r2,el('label',{class:'chk'},c,document.createTextNode(' '+T('maUseC1'))))}
  const go=el('button',{class:'btn',type:'button',text:T('maCalcLr'),id:'maCalcLr'});go.addEventListener('click',async()=>{
   const db=popdbs().find(x=>x.data.database===res.stats.database);if(!db)return message(T('maNoPopDb'));const refs=[L.r1,L.r2].filter(Boolean).map(id=>knowns.find(p=>p.id===id)).filter(Boolean);if(!refs.length)return message(T('maPickRef'));
   const req={action:'lr',population_db:db.data,populations:res.stats.populations,references:refs.map(p=>({id:p.id,markers:p.markers}))};
   if(two)Object.assign(req,{kind:'two',use_c1_only:L.useC1,rows:res.extraction.rows.filter(r=>r.selected)});else Object.assign(req,{kind:'single',markers:alleles(s,res),options:res.options||{}});
   try{L.result=await ctx.post('/api/mixture',req);L.checked=new Set(L.result.hypotheses);draw()}catch(e){message(e.message)}});
  bar.append(go);page.append(bar);
  const r=L.result;if(!r){page.append(el('p',{class:'note',text:T('maLrNote')}));return}
  const names={R1:L.r1,R2:L.r2};const st2=[];
  for(const [k,v] of Object.entries(r.references)){if(k==='mismatch')continue;const id=v.id||names[k];const ok=v.match??(v==='match');
   st2.push(ok?fmt('maRefFound',id,(v.positions||['c1']).map(x=>x==='c1'?'C1 (Major)':'C2 (Minor)').join(', ')):fmt('maRefNotFound',id,s.name)+(v.mismatch_c1?' '+fmt('maRefMismatch',v.mismatch_c1.join(', ')||'—',v.mismatch_c2.join(', ')||'—'):''))}
  if(r.not_in_reference?.length)st2.push(T('maNotInRef')+' '+r.not_in_reference.join(', '));
  page.append(el('div',{class:'note',text:st2.join('\n')}));
  if(!r.hypotheses.length){page.append(el('p',{text:T('maNoHyp')}));return}
  const hb=el('div',{class:'mahyp'});for(const h of r.hypotheses){const [hp,hd]=hypText(h,L.r1,L.r2);const c=el('input',{type:'checkbox','data-h':h});c.checked=L.checked.has(h);c.addEventListener('change',()=>{c.checked?L.checked.add(h):L.checked.delete(h);draw()});
   hb.append(el('label',{class:'chk'},c,document.createTextNode(` Hp = ${hp};  Hd = ${hd}`)))}
  page.append(el('div',{class:'cap',text:T('maHypotheses')}),hb);
  const rows=r.results.filter(x=>L.checked.has(x.hypothesis));const big=v=>v==null?'N/A':v>=1e6?Number(v).toExponential(3):Number(v).toLocaleString(undefined,{maximumFractionDigits:2});
  const t=el('table',{class:'g',id:'maLrTable'});t.append(el('thead',{},el('tr',{},...[T('maPopulation'),'Hp','Hd','Pr(E|Hp)','Pr(E|Hd)','LR',T('maMissingPop')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const x of rows){const [hp,hd]=hypText(x.hypothesis,L.r1,L.r2);tb.append(el('tr',{},el('td',{text:x.population}),el('td',{text:hp}),el('td',{text:hd}),el('td',{class:'num',text:x.numerator.toExponential(4)}),el('td',{class:'num',text:x.denominator.toExponential(4)}),el('td',{class:'num',text:big(x.lr)}),el('td',{text:x.missing_markers.join(', ')})))}
  t.append(tb);page.append(el('div',{class:'cap',text:T('maLrSummary')}),el('div',{class:'grid'},t));
  const ex=el('button',{class:'btn',type:'button',text:T('maExportLr'),id:'maExportLr'});ex.addEventListener('click',()=>{const out=[['Sample File',s.file],['Sample Name',s.name],['Run Folder',s.runFolder],['Panel',s.panel],['Date/Time',new Date().toLocaleString()],['Database',res.stats.database],['Reference 1',L.r1],['Reference 2',L.r2]];
   if(res.extraction?.average_mx)out.push(['Average Mx',res.extraction.average_mx],['Residual Threshold',res.extraction.residual_threshold]);
   for(const x of rows){const [hp,hd]=hypText(x.hypothesis,L.r1,L.r2);out.push([],['Population',x.population,'Theta',x.theta],['Hp',hp],['Hd',hd],['Marker','Hp','Hd']);
    for(const d of x.details)out.push(d.note?[d.marker,d.note]:[d.marker,d.hp,d.hd]);out.push(['Likelihood Ratio',x.lr,`${x.numerator}/${x.denominator}`]);if(x.missing_markers.length)out.push(['Markers missing in population',x.missing_markers.join(' ')])}
   download(`${s.name} - LR Statistics.csv`,csv(out))});
  page.append(el('div',{class:'btnrow'},ex))}
 // ---------- merged results (samples with the same name) ----------
 function mergeCandidates(){const by={};for(const id of st.order){if(id.startsWith('merged:'))continue;const s=sOf(id),r=st.results[id];if(!r||r.contributors>2)continue;
   if(r.contributors===2&&!(r.review&&r.extraction?.status==='extracted'))continue;(by[s.name+'\u0001'+r.contributors]=by[s.name+'\u0001'+r.contributors]||[]).push(id)}
  return Object.entries(by).filter(([,v])=>v.length>1)}
 async function mergeGroups(){const groups=mergeCandidates();if(!groups.length)return message(T('maNoMerge'));st.virtual=st.virtual||{};let first=null;
  for(const [key,ids] of groups){const [name,c]=key.split('\u0001');const contributors=+c;const vid=`merged:${name}:${c}`;
   const sources=ids.map(id=>{const s=sOf(id),r=st.results[id];return {id,markers:contributors===2?[...new Set(r.extraction.rows.map(x=>x.marker))].map(m=>({name:m,rows:r.extraction.rows.filter(x=>x.marker===m)})):Object.entries(alleles(s,r)).map(([m,a])=>({name:m,alleles:a}))}});
   try{const merged=await ctx.post('/api/mixture',{action:'merge',kind:contributors===2?'two':'one',sources});
    const base=sOf(ids[0]);st.virtual[vid]={key:vid,file:vid,name:`${name} (${T('maMergedName')})`,runFolder:'<Multiple>',panel:[...new Set(ids.map(i=>sOf(i).panel))].join(', '),markers:[],virtual:true};
    st.results[vid]={contributors,merged,method:st.results[ids[0]].method,mit:0,sources:ids,options:{},extraction:{status:'extracted',rows:[],residual_threshold:0.04,average_mx:null,missing_markers:[]}};
    deriveMerged(st.virtual[vid],st.results[vid]);if(!st.order.includes(vid))st.order.push(vid);first=first||vid}catch(e){return message(e.message)}}
  st.current=first;st.vtab='results';draw()}
 function deriveMerged(s,res){const m=res.merged.markers;
  if(res.contributors===2){res.extraction.rows=m.flatMap(x=>x.chosen==null?[]:x.variants[x.chosen].rows.map(r=>({...r,selected:true,inconclusive:{...(r.inconclusive||{c1:false,c2:false})}})));
   res.extraction.missing_markers=m.filter(x=>x.chosen==null).map(x=>x.marker)}
  else s.markers=m.filter(x=>x.chosen!=null).map(x=>({name:x.marker,y:false,alleles:x.variants[x.chosen].alleles.map(a=>({label:a,height:1}))}));
  res.stats=null;res.lr=null}
 function mergedView(page,s,res){const m=res.merged.markers;const two=res.contributors===2;
  page.append(el('p',{class:'note',text:fmt('maMergedNote',res.sources.map(id=>sOf(id).file).join(', '))}));
  const t=el('table',{class:'g matab'});t.append(el('thead',{},el('tr',{},...[T('colMarker'),T('maMergeStatus'),T('maMergeSource'),...(two?['C1 (Major)','C2 (Minor)']:[T('maAlleles')])].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const x of m){const srcCell=el('td');
   if(x.status==='conflict'){const sel=el('select',{'data-marker':x.marker});sel.append(new Option(T('maChoose'),''));x.variants.forEach((v,i)=>sel.append(new Option(sOf(v.source).file,String(i))));sel.value=x.chosen==null?'':String(x.chosen);
    sel.addEventListener('change',()=>{x.chosen=sel.value===''?null:+sel.value;deriveMerged(s,res);draw()});srcCell.append(sel)}else srcCell.textContent=x.source==='<Multiple>'?'<Multiple>':sOf(x.source).file;
   const v=x.chosen==null?null:x.variants[x.chosen];
   const cells=two?[el('td',{text:v?v.rows.map(r=>gt(r.c1)).join(' | '):''}),el('td',{text:v?v.rows.map(r=>gt(r.c2)).join(' | '):''})]:[el('td',{text:v?v.alleles.join(', '):''})];
   tb.append(el('tr',{class:x.status==='conflict'&&x.chosen==null?'conflict':''},el('td',{text:x.marker}),el('td',{html:pqv(x.status==='conflict'?(x.chosen==null?'low':'check'):'pass'),title:T('maMerge_'+x.status)}),srcCell,...cells))}
  t.append(tb);page.append(el('div',{class:'grid magrid'},t));if(!two)oneContributor(page,s,res)}
 function exportResults(s,res){const head=[`Sample File,${s.file}`,`Sample Name,${s.name}`,`Run Folder,${s.runFolder}`,`Panel,${s.panel}`,`Mixture Analysis Method,${res.method}`,`Date/Time,${new Date().toLocaleString()}`].join('\r\n')+'\r\n';
  if(res.contributors===2&&res.extraction?.rows){const x=res.extraction;const code=v=>({pass:0,check:1,low:2,na:-2}[v]??-2);
   const rows=[['Table','Marker','C1 (Major)','C2 (Minor)','Inconclusive C1','Inconclusive C2','ADBI','Mx','Residual','PHR1','PHR2','Residual Status','PHR Status','IQ']];
   for(const r of x.rows)rows.push([r.selected?'Selected':'Unselected',r.marker,gt(r.c1),gt(r.c2),r.inconclusive.c1?'TRUE':'FALSE',r.inconclusive.c2?'TRUE':'FALSE',code(r.adbi),r.mx||'N/A',r.residual.toFixed(6),r.phr1??'N/A',r.phr2??'N/A',code(r.residual_status),code(r.phr_status),code(r.iq)]);
   download(`${s.name} - 2 Contributors.csv`,head+`Average Mx,${x.average_mx}\r\nResidual Threshold,${x.residual_threshold}\r\n`+csv(rows))}
  else download(`${s.name} - ${res.contributors===1?'1 Contributor':'3+ Contributors'}.csv`,head+csv([['Marker','Alleles','Option'],...Object.entries(alleles(s,res)).filter(([,a])=>a.length).map(([m,a])=>[m,a.join(', '),res.options?.[m]||''])]))}
 async function params(res){const m=await getMethod(res.method);message(`${m.name}\n${m.description||''}\nMIT: ${m.mit} RFU\n${T('maPhrSettings')}:\n`+m.phr.map(r=>`${r[0]}–${r[1]} RFU: ${r[2]}`).join('\n'))}
 draw();setTimeout(()=>{},0)}

window.Mixture={init(c){ctx=c},open,manager,methods,popdbs};
})();
