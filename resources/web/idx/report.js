/* Hot keys follow the physical key (works with a Russian layout: Ctrl+В is Ctrl+D). */
var hotKey=e=>/^Key[A-Z]$/.test(e.code||'')?e.code.slice(3).toLowerCase():String(e.key||'').toLowerCase();
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
 const set=currentSetting();let d;
 if(set.steps&&set.steps.some(x=>x.active!==false)){const full=clone(set);full.columns.forEach(c=>c.show=true);d=o.report.data(full,st.files);
  d.cols.forEach((c,i)=>{c.show=set.columns[i].show});d=ReportSettings.applySteps(set,d)}
 else d=o.report.data(set,st.files);
 st.files=d.files;return d}
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
 t.append(el('thead',{},el('tr',{},el('th',{class:'rh'}),...m[0].map((h,j)=>{const th=el('th',{text:String(h)+(!st.flip&&!st.byMarker&&st.sortCol===j?(st.sortDir>0?' ▲':' ▼'):''),title:T('rpHeaderHint')});th.addEventListener('click',e=>headerClick(e,j));return th}))));
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
 t.append(tb);g.append(t);if(st.selCol!=null&&st.selCol<(m[0]||[]).length){t.querySelectorAll('thead th').forEach((x,k)=>x.classList.toggle('colsel',k===st.selCol+1));t.querySelectorAll('tbody tr').forEach(tr=>tr.children[st.selCol+1]?.classList.add('colsel'))}
 $('statusText').textContent=`${m.length-1} ${T('rpRows')}; ${d.files.length} ${T('samples').toLowerCase()}`}
// Column header, as in ID-X ReportManagerPanel: click selects the column, Shift+click sorts by it (again: reverses), Ctrl+click shows what the column is.
function headerClick(e,j){const d=st.data;if(!d)return;const editable=!st.flip&&!st.byMarker;
 if(e.ctrlKey||e.metaKey){const c=editable?d.cols[j]:null;message(`${d.heads[editable?j:0]??''}${c?' — '+T('rsCat'+c.cat):''}`);return}
 if(e.shiftKey){if(!editable)return;st.sortDir=st.sortCol===j?-st.sortDir:1;st.sortCol=j;const c=d.cols[j];
  sortBy(j,st.sortDir);return}
 st.selCol=j;st.sel=new Set(Array.from({length:d.rows.length},(_,k)=>k));render()}
// Edit > Find / Sort / Delete Row(s) / Cut / Paste / Clear, View Default Table (ID-X ReportManager).
function sortBy(j,dir){const d=st.data;if(!d||st.flip||st.byMarker)return;st.sortCol=j;st.sortDir=dir;const c=d.cols[j];
 const val=r=>c.cat==='Custom'?(st.custom[rowKey(r)]?.[c.id]??''):r.values[j];
 d.rows.sort((a,b)=>{const x=val(a),y=val(b);const r=(typeof x==='number'&&typeof y==='number')?x-y:String(x??'').localeCompare(String(y??''),undefined,{numeric:true});return r*dir});
 st.sel=new Set();st.anchor=null;render()}
function sortDialog(){const d=st.data;if(!d||st.flip||st.byMarker)return message(T('rpSortHint'));
 dialog(T('sort').replace(/\.+$/,''),b=>{const col=el('select',{id:'rpSortCol'});d.heads.forEach((h,j)=>col.append(new Option(h,String(j))));col.value=String(st.sortCol??0);
  const dir=el('select',{id:'rpSortDir'},new Option(T('ascending'),'1'),new Option(T('descending'),'-1'));dir.value=String(st.sortDir??1);
  b.append(el('div',{class:'field'},el('label',{text:T('colName')}),col),el('div',{class:'field'},el('label',{text:T('sortOrder')}),dir))},
  [[T('ok'),bd=>{sortBy(+bd.querySelector('#rpSortCol').value,+bd.querySelector('#rpSortDir').value);return true},true],[T('cancel'),()=>true]])}
function findDialog(){const d=st.data;if(!d)return;
 dialog(T('find').replace(/\.+$/,''),b=>b.append(el('div',{class:'field'},el('label',{text:T('findText')}),el('input',{type:'text',id:'rpFindText',value:st.findText||''}))),
  [[T('findNext'),bd=>{const t=bd.querySelector('#rpFindText').value.trim().toLowerCase();st.findText=t;if(!t)return false;
    const m=viewMatrix().slice(1);const start=(st.findAt??-1)+1;
    for(let k=0;k<m.length;k++){const i=(start+k)%m.length;if(m[i].some(v=>String(v??'').toLowerCase().includes(t))){st.findAt=i;st.sel=new Set([i]);st.anchor=i;render();
      document.querySelector(`#reportGrid tr[data-i="${i}"]`)?.scrollIntoView({block:'center'});return false}}
    message(T('findNone'));return false}],[T('closeWin'),()=>true,true]])}
function defaultTable(){st.flip=false;st.byMarker=false;render()}
function deleteRows(){const d=st.data;if(!d||st.flip||st.byMarker||!st.sel.size)return message(T('selectRow'));
 d.rows=d.rows.filter((_,i)=>!st.sel.has(i));st.sel=new Set();st.anchor=null;st.dirty=true;render()}
const custCol=()=>{const f=st.focus;return f&&st.data?.cols.find(c=>c.id===f.col&&c.cat==='Custom')?f.col:null};
function clearCells(cut){const col=custCol();if(!col||!st.sel.size)return message(T('rpClearHint'));
 const rows=[...st.sel].sort((a,b)=>a-b);if(cut)navigator.clipboard?.writeText(rows.map(i=>st.custom[rowKey(st.data.rows[i])]?.[col]??'').join('\n'));
 for(const i of rows){const k=rowKey(st.data.rows[i]);st.custom[k]={...(st.custom[k]||{}),[col]:''}}st.dirty=true;render()}
async function pasteCells(){const col=custCol();if(!col||!st.sel.size)return message(T('rpClearHint'));let text='';try{text=await navigator.clipboard.readText()}catch(e){return message(T('pasteDenied'))}
 const lines=text.replace(/\r/g,'').split('\n');while(lines.length&&lines[lines.length-1]==='')lines.pop();if(!lines.length)return;
 [...st.sel].sort((a,b)=>a-b).forEach((i,n)=>{const l=lines.length===1?lines[0]:lines[n];if(l==null)return;const k=rowKey(st.data.rows[i]);st.custom[k]={...(st.custom[k]||{}),[col]:String(l).split('\t')[0]}});
 st.dirty=true;render()}
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
async function saveReport(asNew){const o=api();if(!o||!st.data)return;const name=(asNew!==true&&st.name)||await askName(st.name||currentSetting().name);if(!name)return;
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
  {label:T('file'),items:[{label:T('rpOpen'),sc:'Ctrl+O',run:openReport},{label:T('rpSave'),sc:'Ctrl+S',run:()=>saveReport()},{label:T('saveAs'),run:()=>saveReport(true)},'-',{label:T('rpExport'),sc:'Ctrl+E',run:exportDialog},'-',
   {label:T('rpByMarker'),checked:st.byMarker,run:()=>{st.byMarker=!st.byMarker;if(st.byMarker)st.flip=false;render()}},'-',
   {label:T('printPreview'),run:printPreview},{label:T('print'),sc:'Ctrl+P',run:printNow},'-',{label:T('closeWin'),run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('cut'),sc:'Ctrl+X',run:()=>clearCells(true)},{label:T('copy'),sc:'Ctrl+C',run:copy},{label:T('paste'),sc:'Ctrl+V',run:pasteCells},{label:T('clear'),sc:'Delete',run:()=>clearCells(false)},'-',
   {label:T('selectAll'),sc:'Ctrl+A',run:()=>{st.sel=new Set((st.data?.rows||[]).map((_,i)=>i));render()}},'-',{label:T('fillDown'),sc:'Ctrl+D',run:fillDown},{label:T('rpDefaultTable'),run:defaultTable},'-',
   {label:T('find'),sc:'Ctrl+F',run:findDialog},{label:T('sort'),sc:'Ctrl+G',run:sortDialog},'-',{label:T('deleteRows'),run:deleteRows}]},
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
 if(e.key==='Delete'&&!e.ctrlKey&&!e.metaKey&&!e.target.closest('input,textarea,select,.dlg')){e.preventDefault();clearCells(false);return}
 if(!(e.ctrlKey||e.metaKey))return;const k=hotKey(e);
 if(e.target.closest('input')&&k!=='d')return;
 const map={o:openReport,s:()=>saveReport(),e:exportDialog,c:copy,x:()=>clearCells(true),v:pasteCells,f:findDialog,g:sortDialog,d:fillDown,p:printNow,l:plots,a:()=>{st.sel=new Set((st.data?.rows||[]).map((_,i)=>i));render()}};if(map[k]){e.preventDefault();map[k]()}});
window.addEventListener('storage',e=>{if(e.key===ReportSettings.KEY){if(st.setting)st.setting=ReportSettings.get(st.setting.name)||st.setting;refresh(true)}});
window.addEventListener('beforeunload',e=>{if(st.dirty){e.preventDefault();e.returnValue=''}});
refresh(false);
})();
