// Report Manager window: table-formatted reports built with Report Settings from the
// project window's selected samples; custom columns, flip, table by marker, export, print, save in project.
(function(){
const $=id=>document.getElementById(id);
const api=()=>{try{return window.opener&&!window.opener.closed?window.opener.OT:null}catch(e){return null}};
const st={setting:null,files:null,custom:{},flip:false,byMarker:false,name:null,dirty:false,data:null,sel:new Set(),anchor:null};
try{const l=api()?.state?.project&&window.opener.I18N?.lang;if(l)I18N.set(l)}catch(e){}
const clone=x=>JSON.parse(JSON.stringify(x));
const rowKey=r=>r.file+'\u0001'+r.marker;
function currentSetting(){return st.setting||ReportSettings.get(ReportSettings.names()[0])}
function compute(){const o=api();if(!o||!o.report)return null;
 const d=o.report.data(currentSetting(),st.files);st.files=d.files;return d}
function refresh(keepFiles){if(!keepFiles)st.files=null;st.data=compute();st.sel.clear();render()}
// Current view as a matrix [header,...rows] of plain values (custom cells included).
function viewMatrix(){const d=st.data;if(!d)return [[]];
 if(st.byMarker){const head=['Marker',...d.byMarker.samples.map(s=>s.name)];
  return [head,...d.byMarker.markers.map(m=>[m,...d.byMarker.samples.map(s=>(s.alleles[m]||[]).join(', '))])]}
 const head=d.heads;const rows=d.rows.map(r=>d.cols.map((c,i)=>c.cat==='Custom'?(st.custom[rowKey(r)]?.[c.id]??''):r.values[i]));
 if(!st.flip)return [head,...rows];
 return head.map((h,i)=>[h,...rows.map(r=>r[i])])}
function title(){return `${T('reportManager').replace(/\.+$/,'')} - ${st.dirty?'*':''}${st.name||T('untitled')}`}
function render(){
 document.title=title();buildChrome();
 const d=st.data;const g=$('reportGrid');g.replaceChildren();
 $('rptitle').textContent=d?`${d.project} — ${currentSetting()?.name||''}${st.flip?' — '+T('rpFlip'):''}${st.byMarker?' — '+T('rpByMarker'):''}`:T('rpNoProject');
 if(!d){$('statusText').textContent=T('rpNoProject');return}
 const t=el('table',{class:'g'});const m=viewMatrix();
 t.append(el('thead',{},el('tr',{},el('th',{class:'rh'}),...m[0].map(h=>el('th',{text:String(h)})))));
 const tb=el('tbody');const editable=!st.flip&&!st.byMarker;
 m.slice(1).forEach((row,i)=>{const tr=el('tr',{class:st.sel.has(i)?'sel':''});tr.dataset.i=String(i);tr.append(el('td',{class:'rh',text:String(i+1)}));
  row.forEach((v,j)=>{const c=editable?d.cols[j]:null;
   if(c&&c.cat==='Custom'){const r=d.rows[i];const inp=el('input',{type:'text',value:v??'','data-col':c.id});
    inp.addEventListener('input',()=>{const k=rowKey(r);st.custom[k]={...(st.custom[k]||{}),[c.id]:inp.value};st.dirty=true;document.title=title()});
    inp.addEventListener('focus',()=>{st.focus={col:c.id,i}});tr.append(el('td',{class:'cust'},inp))}
   else{const td=el('td',{text:v==null?'':String(v)});if(typeof v==='number')td.className='num';tr.append(td)}});
  tr.addEventListener('mousedown',e=>{if(e.target.tagName==='INPUT')return;
   if(e.shiftKey&&st.anchor!=null){const [a,b]=[Math.min(st.anchor,i),Math.max(st.anchor,i)];st.sel=new Set(Array.from({length:b-a+1},(_,k)=>a+k))}
   else if(e.ctrlKey||e.metaKey){st.sel.has(i)?st.sel.delete(i):st.sel.add(i);st.anchor=i}else{st.sel=new Set([i]);st.anchor=i}
   tb.querySelectorAll('tr').forEach(x=>x.classList.toggle('sel',st.sel.has(Number(x.dataset.i))))});
  tb.append(tr)});
 t.append(tb);g.append(t);
 $('statusText').textContent=`${m.length-1} ${T('rpRows')}; ${d.files.length} ${T('samples').toLowerCase()}`}
function fillDown(){const f=st.focus;if(!f||!st.data)return message(T('rpFillHint'));const d=st.data;
 const v=st.custom[rowKey(d.rows[f.i])]?.[f.col]??'';for(let i=f.i+1;i<d.rows.length;i++){const k=rowKey(d.rows[i]);st.custom[k]={...(st.custom[k]||{}),[f.col]:v}}
 st.dirty=true;render()}
function copy(){const text=viewMatrix().map(r=>r.map(x=>x==null?'':String(x)).join('\t')).join('\n');
 (navigator.clipboard?navigator.clipboard.writeText(text):Promise.reject()).then(()=>{$('statusText').textContent=T('copied')},()=>message(text))}
const csvCell=x=>{const s=x==null?'':String(x);return /[",\r\n]/.test(s)?'"'+s.replace(/"/g,'""')+'"':s};
const ser=(rows,type)=>rows.map(r=>r.map(x=>type==='csv'?csvCell(x):(x==null?'':String(x).replace(/[\t\r\n]+/g,' '))).join(type==='csv'?',':'\t')).join('\r\n')+'\r\n';
function save(name,text,type){const url=URL.createObjectURL(new Blob([text],{type:type==='csv'?'text/csv':'text/plain'}));const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
function exportDialog(){if(!st.data)return;let type='txt',fmtMode='apply',sep='group';
 dialog(T('rpExport'),b=>{b.classList.add('ecdlg');
  const radio=(name,value,label,cur,set)=>{const r=el('input',{type:'radio',name,value,id:`rp_${name}_${value}`});r.checked=cur===value;r.addEventListener('change',()=>{if(r.checked)set(value)});return el('label',{class:'chk'},r,document.createTextNode(' '+label))};
  const ts=el('select',{id:'rpType'});ts.append(new Option(T('typeTxt'),'txt'),new Option(T('typeCsv'),'csv'),new Option('PDF','pdf'));ts.addEventListener('change',()=>type=ts.value);
  b.append(el('div',{class:'field'},el('label',{text:T('exportType')}),ts),el('div',{class:'field'},el('label',{text:T('fileName')}),el('input',{type:'text',id:'rpName',value:(st.name||currentSetting().name).replace(/[\\/:*?"<>|]+/g,'_')})),
   el('fieldset',{},el('legend',{text:T('rpFormat')}),radio('fmt','apply',T('rpApply'),fmtMode,v=>fmtMode=v),radio('fmt','allele',T('ecAlleleTable'),fmtMode,v=>fmtMode=v),
    el('div',{class:'ind'},radio('sep','group',T('ecGroup'),sep,v=>sep=v),radio('sep','separate',T('ecSeparate'),sep,v=>sep=v))),el('p',{class:'note',text:T('rpPdfNote')}))},
  [[T('exportBtn'),b=>{const name=b.querySelector('#rpName').value.trim()||'report';
    if(type==='pdf'){st.byMarker=true;render();printNow();return true}
    if(fmtMode==='apply'){save(`${name}.${type}`,ser(viewMatrix(),type),type);return true}
    const files=api().report.alleleTable(currentSetting(),st.files,sep==='separate');const names=[];
    for(const f of files){const n=`${name}${f.panel?'_'+f.panel.replace(/[\\/:*?"<>|]+/g,'_'):''}.${type}`;save(n,ser(f.rows,type),type);names.push(n)}
    message(T('ecFilesCreated')+'\n'+names.join('\n'));return true},true],[T('cancel'),()=>true]])}
function askName(def){return new Promise(res=>{dialog(T('rpSave'),b=>b.append(el('div',{class:'field'},el('label',{text:T('rpReportName')}),el('input',{type:'text',id:'rpSaveName',value:def||''}))),
 [[T('ok'),b=>{const v=b.querySelector('#rpSaveName').value.trim();if(!v||v.length>120){message(T('enterName'));return false}res(v);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
async function saveReport(){const o=api();if(!o||!st.data)return;const name=await askName(st.name||currentSetting().name);if(!name)return;
 o.report.save({name,setting:clone(currentSetting()),files:st.files,custom:st.custom,flip:st.flip,byMarker:st.byMarker});st.name=name;st.dirty=false;render();$('statusText').textContent=T('rpSaved')+': '+name}
function openReport(){const o=api();if(!o)return;let sel=null;
 const d=dialog(T('rpOpen'),()=>{},[[T('open'),()=>{const r=sel&&o.report.get(sel);if(!r){message(T('selectRow'));return false}
   st.setting=ReportSettings.get(r.setting.name)||r.setting;st.files=r.files;st.custom=clone(r.custom||{});st.flip=!!r.flip;st.byMarker=!!r.byMarker;st.name=r.name;st.dirty=false;st.data=compute();render();return true},true],
  [T('mgrDelete'),()=>{if(sel){o.report.remove(sel);sel=null;draw()}return false}],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('rpReportName'),T('reportSetting'),T('samples'),T('colLastSaved')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');for(const r of o.report.list()){const tr=el('tr',{class:r.name===sel?'sel':''},el('td',{text:r.name}),el('td',{text:r.setting}),el('td',{class:'num',text:String(r.samples)}),el('td',{text:new Date(r.saved).toLocaleString()}));
   tr.addEventListener('mousedown',()=>{sel=r.name;tb.querySelectorAll('tr').forEach(x=>x.classList.toggle('sel',x===tr))});tb.append(tr)}
  t.append(tb);b.append(el('div',{class:'grid mgrgrid'},t));b.classList.add('rpopen')};
 draw()}
function plots(){const o=api();if(!o||!st.data)return;const d=st.data;let files;
 if(st.byMarker||st.flip)files=d.files;else{files=[...new Set([...st.sel].map(i=>d.rows[i]?.file).filter(Boolean))];if(!files.length)return message(T('selectSamplesFirst'))}
 o.report.plot(files)}
function editSetting(){const s=currentSetting();ReportSettings.openEditor(s,{onSave:(ns,prev)=>{ReportSettings.put(ns,prev);st.setting=ns;refresh(true)}})}
function printPayload(){const m=viewMatrix();let page={};try{page=window.opener.OT.printPayload().page}catch(e){}
 return {title:currentSetting()?.name||T('reportManager'),subtitle:st.name||'',project:st.data?.project||'',date:new Date().toLocaleString(),page,table:m,lang:I18N.lang}}
window.OT={printPayload};
function printPreview(){const w=window.open('/idx/print.html','opentrace-print-report','width=1000,height=760');if(w)w.focus()}
function printNow(){window.open('/idx/print.html#print','opentrace-print-report','width=1000,height=760')}
function buildChrome(){
 buildMenubar($('menubar'),[
  {label:T('file'),items:[{label:T('rpOpen'),sc:'Ctrl+O',run:openReport},{label:T('rpSave'),sc:'Ctrl+S',run:saveReport},'-',{label:T('rpExport'),sc:'Ctrl+E',run:exportDialog},'-',
   {label:T('rpByMarker'),checked:st.byMarker,run:()=>{st.byMarker=!st.byMarker;if(st.byMarker)st.flip=false;render()}},'-',
   {label:T('printPreview'),run:printPreview},{label:T('print'),sc:'Ctrl+P',run:printNow},'-',{label:T('closeWin'),run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('copy'),sc:'Ctrl+C',run:copy},{label:T('fillDown'),sc:'Ctrl+D',run:fillDown},{label:T('selectAll'),sc:'Ctrl+A',run:()=>{st.sel=new Set((st.data?.rows||[]).map((_,i)=>i));render()}}]},
  {label:T('view'),items:[{label:T('rpRefresh'),sc:'F5',run:()=>refresh(false)},{label:T('rpFlip'),checked:st.flip,run:()=>{st.flip=!st.flip;if(st.flip)st.byMarker=false;render()}},
   {label:T('rpByMarker'),checked:st.byMarker,run:()=>{st.byMarker=!st.byMarker;if(st.byMarker)st.flip=false;render()}},'-',{label:T('displayPlots'),sc:'Ctrl+L',run:plots}]},
  {label:T('tools'),items:[{label:T('reportSettingsEditor'),run:editSetting}]}]);
 const tb=$('toolbar');tb.replaceChildren();
 const sel=el('select',{class:'combo',id:'rpSetting'});for(const n of ReportSettings.names())sel.append(new Option(n,n));
 if(st.setting&&!ReportSettings.get(st.setting.name))sel.append(new Option(st.setting.name,st.setting.name));
 sel.value=currentSetting()?.name||'';sel.addEventListener('change',()=>{st.setting=ReportSettings.get(sel.value);st.dirty=true;refresh(true)});
 tb.append(el('span',{class:'tlabel',text:T('reportSetting')}),sel,tbutton('reportSettingsEditor',T('reportSettingsEditor'),editSetting,{id:'rpEditSetting'}),tsep(),
  tbutton('displayPlots',T('displayPlots'),plots,{id:'rpPlots'}),tbutton('rpRefresh',T('rpRefresh'),()=>refresh(false),{id:'rpRefreshBtn'}),tsep(),
  tbutton('exportTable',T('rpExport'),exportDialog,{id:'rpExportBtn'}),tbutton('printPreview',T('printPreview'),printPreview,{id:'rpPreview'}),tbutton('print',T('print'),printNow,{id:'rpPrint'}))}
document.addEventListener('keydown',e=>{
 if(e.key==='F5'){e.preventDefault();refresh(false);return}
 if(!(e.ctrlKey||e.metaKey))return;const k=e.key.toLowerCase();
 if(e.target.closest('input')&&k!=='d')return;
 const map={o:openReport,s:saveReport,e:exportDialog,c:copy,d:fillDown,p:printNow,l:plots};if(map[k]){e.preventDefault();map[k]()}});
window.addEventListener('storage',e=>{if(e.key===ReportSettings.KEY){if(st.setting)st.setting=ReportSettings.get(st.setting.name)||st.setting;refresh(true)}});
window.addEventListener('beforeunload',e=>{if(st.dirty){e.preventDefault();e.returnValue=''}});
refresh(false);
})();
