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
 const st={page:'select',methodOf:{},include:new Set(eligible.map(s=>s.file)),seg:null,tab:2,known:{},current:null,results:saved};
 for(const s of eligible)st.methodOf[s.file]=saved[s.file]?.method||meths[0].name;
 const d=dialog(T('mixture').replace(/\.+$/,''),()=>{},[]);d.body.classList.add('madlg');
 const foot=d.root.querySelector('.foot');
 const setButtons=list=>{foot.replaceChildren();for(const [label,fn,id,dis] of list){const b=el('button',{class:'btn',type:'button',text:label,id});b.disabled=!!dis;b.addEventListener('click',fn);foot.append(b)}};
 const finish=()=>{ctx.saveResults(st.results);d.close()};
 const draw=()=>{const b=d.body;b.replaceChildren();
  if(st.page==='select'){b.append(el('h3',{text:T('maSelectTitle')}),el('p',{class:'note',text:T('maSelectNote')}));
   const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...['',T('sumRunFolder'),T('colSampleFile'),T('colSampleName'),T('maMethod')].map(h=>el('th',{text:h})))));const tb=el('tbody');
   for(const s of eligible){const cb=el('input',{type:'checkbox','data-file':s.file});cb.checked=st.include.has(s.file);cb.addEventListener('change',()=>{cb.checked?st.include.add(s.file):st.include.delete(s.file)});
    const ms=el('select');for(const m of meths)ms.append(new Option(m.name,m.name));ms.value=st.methodOf[s.file];ms.addEventListener('change',()=>{st.methodOf[s.file]=ms.value});
    tb.append(el('tr',{},el('td',{class:'ic'},cb),el('td',{text:s.runFolder}),el('td',{text:s.file}),el('td',{text:s.name}),el('td',{},ms)))}
   t.append(tb);b.append(el('div',{class:'grid magrid'},t));
   setButtons([[T('maDetails'),()=>message(ol.length?T('maOlList')+'\n'+ol.map(s=>s.name).join('\n'):T('maOlNone'))],[T('mixtureManager').replace(/\.+$/,''),()=>manager()],
    [T('cancel'),()=>d.close()],[T('maNext'),()=>segregate(),'maNext']])}
  else if(st.page==='segregate'){const groups={1:[],2:[],3:[]};for(const r of st.seg)if(r.contributors)groups[r.contributors].push(r);
   const tabs=el('div',{class:'tabs dtabs'});for(const k of [1,2,3]){const x=el('div',{class:'tab'+(st.tab===k?' on':''),text:`${T('maTab'+k)} (${groups[k].length})`,'data-tab':String(k)});x.addEventListener('mousedown',()=>{st.tab=k;draw()});tabs.append(x)}
   const t=el('table',{class:'g'});const heads=[T('sumRunFolder'),T('colSampleFile'),T('colSampleName'),T('maMethod'),T('maReason')];if(st.tab===2)heads.unshift(T('maExtraction'),T('maReview'));if(st.tab===2)heads.push(T('maKnown'));
   t.append(el('thead',{},el('tr',{},...heads.map(h=>el('th',{text:h})))));const tb=el('tbody');
   for(const r of groups[st.tab]){const s=all.find(x=>x.file===r.id);const res=st.results[r.id];const cells=[];
    if(st.tab===2)cells.push(el('td',{class:'ic',html:res?.extraction?.status==='extracted'?'':pqv('check')}),el('td',{text:res?.review?'TRUE':'FALSE'}));
    cells.push(el('td',{text:s.runFolder}),el('td',{text:s.file}),el('td',{text:s.name}),el('td',{text:st.methodOf[r.id]}),el('td',{text:reasonText(r)}));
    if(st.tab===2){const ks=el('select');ks.append(new Option(T('maNone'),''));for(const p of ctx.knowns())ks.append(new Option(p.id,p.id));ks.value=st.known[r.id]||'';ks.addEventListener('change',()=>{st.known[r.id]=ks.value});cells.push(el('td',{},ks))}
    tb.append(el('tr',{},...cells))}
   t.append(tb);b.append(el('h3',{text:T('maSegTitle')}),tabs,el('div',{class:'grid magrid'},t),el('p',{class:'note',text:T('maSegNote'+st.tab)}));
   const exp=()=>download('Sample Segregation.csv',csv([[T('maContributors'),T('colSampleFile'),T('colSampleName'),T('maMethod'),T('maReason')],...st.seg.filter(r=>r.contributors).map(r=>[r.contributors===3?'3+':r.contributors,r.id,all.find(x=>x.file===r.id).name,st.methodOf[r.id],reasonText(r)])]));
   setButtons([[T('exportBtn'),exp],[T('maBack'),()=>{st.page='select';draw()}],[T('maFinish'),finish],[T('maNext'),()=>extractAll(groups),'maNext2']])}
  else viewer(b)};
 const reasonText=r=>!r.reason?'':r.reason.startsWith('phr_fail:')?fmt('maReasonPhr',r.reason.slice(9)):T('maReason_'+r.reason);
 async function segregate(){const list=eligible.filter(s=>st.include.has(s.file));if(!list.length)return message(T('selectSamplesFirst'));
  try{const out=[];for(const s of list){const m=await getMethod(st.methodOf[s.file]);const r=(await ctx.post('/api/mixture',{action:'segregate',method:m,samples:[{id:s.file,markers:s.markers}]})).samples[0];out.push(r)}
   st.seg=out;for(const r of out){const prev=st.results[r.id];const mit=(await getMethod(st.methodOf[r.id])).mit;if(!prev||prev.method!==st.methodOf[r.id]||prev.contributors!==r.contributors)st.results[r.id]={method:st.methodOf[r.id],mit,contributors:r.contributors,adbi:r.adbi_markers,reason:r.reason}}
   st.tab=[2,1,3].find(k=>out.some(r=>r.contributors===k))||1;st.page='segregate';draw()}catch(e){message(e.message)}}
 async function extractAll(groups){
  for(const r of groups[2]){const res=st.results[r.id];const kn=st.known[r.id]?ctx.knowns().find(p=>p.id===st.known[r.id]):null;
   if(res.extraction&&res.knownId===(kn?.id||''))continue;
   const s=all.find(x=>x.file===r.id);try{res.extraction=await ctx.post('/api/mixture',{action:'extract',method:await getMethod(res.method),sample:{id:s.file,markers:s.markers},known:kn?kn.markers:null});res.knownId=kn?.id||'';res.review=false}catch(e){message(e.message);return}}
  for(const k of [1,3])for(const r of groups[k]){const res=st.results[r.id];res.options=res.options||{}}
  st.page='viewer';const order=[...groups[2],...groups[1],...groups[3]].map(r=>r.id);st.order=order;st.current=order[0];draw()}
 // ---------- results viewer ----------
 function viewer(b){const id=st.current;const s=all.find(x=>x.file===id);const res=st.results[id];
  const nav=el('div',{class:'mahead'});const sel=el('select',{id:'maSample'});for(const f of st.order)sel.append(new Option(all.find(x=>x.file===f).name,f));sel.value=id;sel.addEventListener('change',()=>{st.current=sel.value;st.vtab=null;draw()});
  const i=st.order.indexOf(id);const prev=el('button',{class:'btn',type:'button',text:'<< '+T('maPrevSample')});prev.disabled=i<=0;prev.addEventListener('click',()=>{st.current=st.order[i-1];st.vtab=null;draw()});
  const next=el('button',{class:'btn',type:'button',text:T('maNextSample')+' >>',id:'maNextSample'});next.disabled=i>=st.order.length-1;next.addEventListener('click',()=>{st.current=st.order[i+1];st.vtab=null;draw()});
  nav.append(el('span',{text:`${T('colSampleFile')}: ${s.file}`}),el('span',{text:`${T('sumRunFolder')}: ${s.runFolder}`}),el('span',{text:`${T('colPanel')}: ${s.panel}`}),el('span',{text:`${T('maContributors')}: ${res.contributors===3?'3+':res.contributors}`}),el('span',{},el('label',{text:T('colSampleName')+': '}),sel),prev,next);
  b.append(nav);
  const tabs=[['results',T(res.contributors===1?'ma1Results':res.contributors===2?'maMixResults':'ma3Results')]];
  if(res.stats?.rmp)tabs.push(['rmp',T('maRmpTab')]);if(res.stats?.cpi)tabs.push(['cpi',T('maCpiTab')]);
  st.vtab=st.vtab&&tabs.some(t=>t[0]===st.vtab)?st.vtab:'results';
  const tb=el('div',{class:'tabs dtabs'});for(const [k,label] of tabs){const x=el('div',{class:'tab'+(st.vtab===k?' on':''),text:label,'data-tab':k});x.addEventListener('mousedown',()=>{st.vtab=k;draw()});tb.append(x)}
  const page=el('div',{class:'tabpage dpage mapage'});b.append(tb,page);
  if(st.vtab==='results'){if(res.contributors===1)oneContributor(page,s,res);else if(res.contributors===2)twoContributors(page,s,res);else threePlus(page,s,res)}
  else if(st.vtab==='rmp')rmpView(page,res);else cpiView(page,res);
  const notes=()=>dialog(T('maNotes'),bb=>{const ta=el('textarea',{rows:'6',cols:'60',id:'maNoteText'});ta.value=res.notes||'';bb.append(ta)},[[T('ok'),bb=>{res.notes=bb.querySelector('#maNoteText').value}],[T('cancel'),()=>true]]);
  setButtons([[T('maNotes'),notes],[T('maCalcStats'),()=>calcStats(s,res),'maCalcStats'],[T('exportBtn'),()=>exportResults(s,res)],[T('maParams'),()=>params(res)],[T('maSamplesPlot'),()=>ctx.plot([s.file])],
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
  st.filter=st.filter||{marker:'All',iq:'all',show:'both'};const f=st.filter;
  const hdr=el('div',{class:'mabar'},el('span',{text:`${T('maAvgMx')}: ${num(x.average_mx,4)}`,id:'maAvgMx'}),el('span',{text:`${T('maResidualThr')}: ${x.residual_threshold}`}),
   el('span',{text:`${T('maKnown')}: ${res.knownId||T('maNone')}`}),el('span',{text:`${T('maKnownMatch')}: ${x.known_match||'N/A'}`}));
  const mk=el('select',{id:'maFilterMarker'});mk.append(new Option(T('maAll'),'All'));for(const m of [...new Set(x.rows.map(r=>r.marker))])mk.append(new Option(m,m));mk.value=f.marker;mk.addEventListener('change',()=>{f.marker=mk.value;draw()});
  const iq=el('select',{id:'maFilterIq'});for(const [v,k] of [['all','maShowAll'],['pass','maShowPass'],['check','maShowCheck'],['low','maShowLow']])iq.append(new Option(T(k),v));iq.value=f.iq;iq.addEventListener('change',()=>{f.iq=iq.value;draw()});
  const show=el('select',{id:'maShow'});for(const [v,k] of [['both','maShowBoth'],['selected','maShowSelected'],['unselected','maShowUnselected']])show.append(new Option(T(k),v));show.value=f.show;show.addEventListener('change',()=>{f.show=show.value;draw()});
  const rev=el('input',{type:'checkbox',id:'maReview'});rev.checked=!!res.review;rev.addEventListener('change',()=>{res.review=rev.checked});
  page.append(hdr,el('div',{class:'mabar'},el('label',{text:T('maFilterMarker')}),mk,el('label',{text:T('maFilterIq')}),iq,el('label',{text:T('maTables')}),show,el('label',{class:'chk'},rev,document.createTextNode(' '+T('maReviewComplete')))));
  const selRows=new Set();
  const table=(selected,title)=>{const rows=x.rows.filter(r=>r.selected===selected&&(f.marker==='All'||r.marker===f.marker)&&(selected||f.iq==='all'||r.iq===f.iq));
   const t=el('table',{class:'g matab'});const heads=[T('colMarker'),'C1 (Major)','C2 (Minor)',T('maInc')+' C1',T('maInc')+' C2','ADBI','Mx',T('maResidual'),'PHR1','PHR2',T('maResStatus'),T('maPhrStatus'),'IQ'];if(x.known_match)heads.push(T('maKnownGt'));
   t.append(el('thead',{},el('tr',{},...heads.map(h=>el('th',{text:h})))));const tb=el('tbody');
   for(const r of rows){const ci=r.inconclusive;const inc=k=>{const c=el('input',{type:'checkbox'});c.checked=!!ci[k];c.disabled=!selected;c.addEventListener('change',()=>{ci[k]=c.checked;res.stats=null});c.addEventListener('mousedown',e=>e.stopPropagation());return el('td',{class:'ic'},c)};
    const tr=el('tr',{class:(r.known_match?'known ':'')},el('td',{text:r.marker}),el('td',{text:gt(r.c1)}),el('td',{text:gt(r.c2)}),inc('c1'),inc('c2'),
     el('td',{class:'ic',html:pqv(r.adbi)}),el('td',{class:'num',text:r.mx?num(r.mx):'N/A'}),el('td',{class:'num',text:num(r.residual,4)}),el('td',{class:'num',text:num(r.phr1)}),el('td',{class:'num',text:num(r.phr2)}),
     el('td',{class:'ic',html:pqv(r.residual_status)}),el('td',{class:'ic',html:pqv(r.phr_status)}),el('td',{class:'ic',html:pqv(r.iq)}));
    if(x.known_match)tr.append(el('td',{text:r.known?.length?r.known.join(', '):''}));
    tr.addEventListener('mousedown',e=>{if(!(e.ctrlKey||e.metaKey))selRows.clear(),tb.querySelectorAll('tr').forEach(z=>z.classList.remove('sel'));selRows.add(r);tr.classList.add('sel')});tb.append(tr)}
   t.append(tb);return el('div',{},el('div',{class:'cap',text:`${title} (${rows.length})`}),el('div',{class:'grid magrid'+(selected?' sel':'')},t))};
  const move=to=>{if(!selRows.size)return message(T('selectRow'));for(const r of selRows){r.selected=to;if(!to)r.inconclusive={c1:false,c2:false}}x.missing_markers=[...new Set(x.rows.map(r=>r.marker))].filter(m=>!x.rows.some(r=>r.marker===m&&r.selected)).sort();res.stats=null;draw()};
  const bar=el('div',{class:'btnrow'});const bt=(text,fn,id)=>{const z=el('button',{class:'btn',type:'button',text,id});z.addEventListener('click',fn);return z};
  bar.append(bt(T('maUnselect'),()=>move(false),'maUnselect'),bt(T('maSelect'),()=>move(true),'maSelectBtn'),bt(T('maMissing'),()=>message(x.missing_markers.length?T('maMissingList')+'\n'+x.missing_markers.join(', '):T('maMissingNone')),'maMissingBtn'));
  if(f.show!=='unselected')page.append(table(true,T('maSelectedTable')));
  page.append(bar);
  if(f.show!=='selected')page.append(table(false,T('maUnselectedTable')))}
 function contributorProfile(res,key){const prof={};for(const r of res.extraction.rows){if(!r.selected)continue;const p=prof[r.marker]=prof[r.marker]||{genotypes:[],inconclusive:false};const g=r[key].map(a=>/^F[123]$/.test(a)?'F':a);if(!p.genotypes.some(x=>[...x].sort().join()===[...g].sort().join()))p.genotypes.push(g);if(r.inconclusive[key])p.inconclusive=true}return prof}
 async function calcStats(s,res){const dbs=popdbs();if(!dbs.length)return message(T('maNoPopDb'));let di=0;const pops=new Set(dbs[0].data.populations.map(p=>p.name));
  dialog(T('maPopSelect'),b=>{const sel=el('select',{id:'maDb'});dbs.forEach((p,i)=>sel.append(new Option(p.data.database,String(i))));const box=el('div',{class:'pcsel',id:'maPops'});
   const fill=()=>{box.replaceChildren();pops.clear();for(const p of dbs[di].data.populations){pops.add(p.name);const c=el('input',{type:'checkbox'});c.checked=true;c.addEventListener('change',()=>{c.checked?pops.add(p.name):pops.delete(p.name)});box.append(el('label',{class:'chk'},c,document.createTextNode(` ${p.name} (θ=${p.theta})`)))}};
   sel.addEventListener('change',()=>{di=+sel.value;fill()});fill();b.append(el('div',{class:'field'},el('label',{text:T('maDatabase')}),sel),el('div',{class:'cap',text:T('maPopulations')}),box,el('p',{class:'note',text:T('maStatsNote')}))},
   [[T('ok'),async()=>{const db=dbs[di].data,names=[...pops];if(!names.length)return message(T('maPickPop')),false;res.stats={database:db.database};
     try{if(res.contributors===1)res.stats.rmp=await ctx.post('/api/mixture',{action:'statistics',kind:'single',population_db:db,populations:names,markers:alleles(s,res),options:res.options});
      if(res.contributors===2){const x=res.extraction;if(x?.status!=='extracted')return message(T('maNotExtracted')),false;
       res.stats.rmp=await ctx.post('/api/mixture',{action:'statistics',kind:'two',population_db:db,populations:names,c1:contributorProfile(res,'c1'),c2:contributorProfile(res,'c2')});
       const drop=[...new Set(x.rows.filter(r=>r.selected&&r.dropout).map(r=>r.marker))];
       res.stats.cpi=await ctx.post('/api/mixture',{action:'statistics',kind:'cpi',population_db:db,populations:names,markers:alleles(s,res),dropout_markers:drop})}
      if(res.contributors===3)res.stats.cpi=await ctx.post('/api/mixture',{action:'statistics',kind:'cpi',population_db:db,populations:names,markers:alleles(s,res)});
      st.vtab=res.stats.rmp?'rmp':'cpi';draw();return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]])}
 const big=v=>v==null?'N/A':v>=1e6?Number(v).toExponential(3):Math.round(v).toLocaleString();
 function rmpView(page,res){const r=res.stats.rmp;const two=r.kind==='two';const t=el('table',{class:'g'});
  t.append(el('thead',{},el('tr',{},...[T('maPopulation'),'θ',...(two?['C1 '+T('maProfileFreq'),'C1 RMP','C2 '+T('maProfileFreq'),'C2 RMP']:[T('maProfileFreq'),'RMP']),T('maMissingPop')].map(h=>el('th',{text:h})))));const tb=el('tbody');
  for(const p of r.results){const cells=two?[p.c1.profile_frequency.toExponential(4),'1 / '+big(p.c1.rmp),p.c2.profile_frequency.toExponential(4),'1 / '+big(p.c2.rmp)]:[p.profile_frequency.toExponential(4),'1 / '+big(p.rmp)];
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
