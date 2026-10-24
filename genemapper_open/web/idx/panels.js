// OpenTrace Panel Manager window: kits, panels, markers, bin sets and marker stutter.
(function(){
const $=id=>document.getElementById(id);
const TOKEN=window.OT_TOKEN;
const DYES=['blue','green','yellow','red','purple','orange'];
const DYE_CSS={blue:'#1f4fd6',green:'#1a9b2e',yellow:'#b59a00',red:'#d32020',purple:'#7a35b5',orange:'#e67e22'};
const REPEATS=[1,2,3,4,5,6,9];

let saved=KitStore.load();
let work=KitStore.clone(saved);
const ui={sel:{type:'root'},binSet:null,selBin:null,selRow:null,dirty:false,view:null,expanded:new Set()};

const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
const kitOf=name=>work.kits.find(k=>k.name===name);
// Kits are data items of a security group: unreadable kits are hidden; Apply checks create/update/delete/control.
const me=()=>window.Sec&&Sec.user?Sec.user.name:'';
const kitCan=(k,op)=>!window.Sec||!Sec.objectCan||Sec.objectCan(k.security_group||'GeneMapper ID-X Security Group',op,k.owner);
const visKits=()=>work.kits.filter(k=>kitCan(k,'read'));
const selKit=()=>ui.sel.kit?kitOf(ui.sel.kit):null;
const selPanel=()=>{const k=selKit();return k&&ui.sel.panel?k.panels.find(p=>p.name===ui.sel.panel):null};
const selMarker=()=>{const p=selPanel();return p&&ui.sel.marker?p.markers.find(m=>m.name===ui.sel.marker):null};
const curSet=()=>{const k=selKit();if(!k)return null;return k.binSets.find(s=>s.name===ui.binSet)||k.binSets[0]||null};
const markerBins=(set,panel,marker)=>set?.panels?.[panel]?.[marker]||null;
const stutterOf=(kit,panel,marker)=>kit.stutter?.panels?.[panel]?.[marker]||[];
function touch(){ui.dirty=true;$('applyBtn').disabled=false}

async function jsonPost(url,data){
 const r=await fetch(url,{method:'POST',headers:{'X-Session-Token':TOKEN,'X-User-Ticket':window.Sec?Sec.ticket():'','Content-Type':'application/json'},body:JSON.stringify(data)});
 const j=await r.json();if(!r.ok)throw Error(j.error||'Error');return j}
function pickText(accept){return new Promise(res=>{const i=el('input',{type:'file',accept:accept||'.txt',style:'display:none'});
 i.addEventListener('change',async()=>{const f=i.files[0];i.remove();res(f?{name:f.name,text:await f.text()}:null)});document.body.append(i);i.click()})}
function download(name,text){const url=URL.createObjectURL(new Blob([text],{type:'text/plain'}));const a=el('a',{href:url,download:name});
 document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
function confirmBox(text,yes){dialog(T('alert'),b=>b.append(el('p',{style:'max-width:440px;white-space:pre-line',text})),
 [[T('ok'),()=>{yes();return true},true],[T('cancel'),()=>true]])}
function ctxMenu(x,y,items){document.querySelectorAll('.menu.ctx').forEach(m=>m.remove());
 const m=el('div',{class:'menu ctx show',style:`left:${x}px;top:${y}px`});
 for(const it of items){if(it==='-'){m.append(el('div',{class:'msep'}));continue}const r=el('div',{class:'mi'+(it.disabled?' dis':''),text:it.label});if(!it.disabled)r.addEventListener('click',()=>{m.remove();it.run()});m.append(r)}
 document.body.append(m);setTimeout(()=>document.addEventListener('mousedown',function h(e){if(!m.contains(e.target)){m.remove();document.removeEventListener('mousedown',h)}}),0)}
function status(text){$('statusText').textContent=text||''}

// ---------- imports ----------
async function importPanels(){
 const f=await pickText();if(!f)return;
 try{const r=await jsonPost('/api/import-gm-kit',{panels:f.text});
  const names=KitStore.addPanels(work,r.panels);const kit=r.panels.kits[0].name;
  ui.sel={type:'kit',kit};ui.expanded.add(kit);touch();render();status(`${f.name}: ${names.length}`);message(T('panelImported'))}
 catch(e){message(T('importErrors')+'\n'+e.message)}}
async function importBins(){
 const kit=selKit();if(!kit)return message(T('selectKitBins'));
 const f=await pickText();if(!f)return;
 try{const r=await jsonPost('/api/import-gm-kit',{bins:f.text});
  const res=KitStore.addBinSet(work,kit.name,r.bins);ui.binSet=res.binSet;touch();render();status(`${f.name}: ${res.binSet}`);message(T('binsImported'))}
 catch(e){message(T('importErrors')+'\n'+e.message)}}
async function importStutter(){
 const kit=selKit();if(!kit)return message(T('selectKitStutter'));
 const go=async()=>{const f=await pickText();if(!f)return;
  try{const r=await jsonPost('/api/import-gm-kit',{stutter:f.text});KitStore.setStutter(work,kit.name,r.stutter);touch();render();status(f.name);message(T('stutterImported'))}
  catch(e){message(T('importErrors')+'\n'+e.message)}};
 if(kit.stutter)confirmBox(T('stutterOverwrite'),go);else go()}
function exportPanels(){const k=selKit();if(!k)return message(T('selectKitFirst'));download(k.name+'_Panels.txt',KitStore.panelsText(k))}
function exportBinSet(){const k=selKit(),s=curSet();if(!k||!s)return message(T('selectBinSetFirst'));download(s.name+'.txt',KitStore.binsText(k,s))}
function exportStutter(){const k=selKit();if(!k||!k.stutter)return message(T('selectKitStutter'));download(k.name+'_stutter.txt',KitStore.stutterText(k))}
function exportAll(){if(!work.kits.length)return;for(const k of work.kits){download(k.name+'_Panels.txt',KitStore.panelsText(k));
 for(const s of k.binSets)download(s.name+'.txt',KitStore.binsText(k,s));if(k.stutter)download(k.name+'_stutter.txt',KitStore.stutterText(k))}}

// ---------- create / delete ----------
const NAME_RE=/^[A-Za-z0-9_.\-+() ]{1,240}$/;
function askName(title,label,initial,done,extra){
 dialog(title,b=>{b.append(el('div',{class:'field'},el('label',{text:label}),el('input',{type:'text',id:'nmIn',value:initial||''})));if(extra)extra(b)},
  [[T('ok'),body=>{const v=body.querySelector('#nmIn').value.trim();if(!v){message(T('enterName'));return false}
    if(!NAME_RE.test(v)){message(T('invalidChars'));return false}return done(v,body)},true],[T('cancel'),()=>true]])}
const groups=()=>window.Sec&&Sec.securityGroups?Sec.securityGroups():['GeneMapper ID-X Security Group'];
function groupField(b,cur){const s=el('select',{id:'nkGroup'});for(const g of groups())s.append(new Option(g,g));s.value=groups().includes(cur)?cur:groups()[0];
 b.append(el('div',{class:'field'},el('label',{text:T('securityGroupLbl')}),s))}
function uniqueName(base,taken){if(!taken.includes(base))return base;for(let i=2;;i++)if(!taken.includes(`${base} ${i}`))return `${base} ${i}`}
function newKit(){askName(T('newKitTitle'),T('kitName'),'',(v,b)=>{if(kitOf(v)){message(fmt('kitExists',v));return false}
 work.kits.push({name:v,type:'MICROSATELLITE',comment:'',security_group:b.querySelector('#nkGroup').value,owner:me(),panelsVersion:'GMID-X v 1.4',panels:[],binSets:[],stutter:null});
 ui.sel={type:'kit',kit:v};touch();render()},
 b=>{b.append(el('div',{class:'field'},el('label',{text:T('kitType')}),el('select',{disabled:''},new Option('Microsatellite','MICROSATELLITE'))));groupField(b)})}
// Duplicate Kit: New Kit dialog; panels, bin sets and marker stutter are copied with the kit.
function duplicateKit(){const k=selKit();if(!k)return message(T('selectKitFirst'));
 askName(T('duplicateKitTitle'),T('kitName'),uniqueName(k.name+' - dup',work.kits.map(x=>x.name)),(v,b)=>{if(kitOf(v)){message(fmt('kitExists',v));return false}
  const c=KitStore.clone(k);c.name=v;c.security_group=b.querySelector('#nkGroup').value;c.owner=me();work.kits.push(c);ui.sel={type:'kit',kit:v};ui.expanded.add(v);touch();render()},
  b=>groupField(b,k.security_group))}
function targetKit(){return selKit()}
// New Panel creates "New Panel" in the kit; the name is then typed in the Panel Name cell (help: Creating a Custom Panel).
function newPanel(){const k=targetKit();if(!k)return message(T('selectKitFirst'));
 const name=uniqueName('New Panel',k.panels.map(p=>p.name));k.panels.push({name,comment:'',markers:[]});
 ui.sel={type:'kit',kit:k.name};ui.expanded.add(k.name);ui.selRow=k.panels.length-1;ui.editCell={row:ui.selRow,col:0};touch();render()}
// Duplicate Panel: "<name> - dup" with its markers, bins of every bin set and marker stutter.
function duplicatePanel(){const k=selKit(),p=selPanel();if(!p)return message(T('selectPanelFirst'));
 const name=uniqueName(p.name+' - dup',k.panels.map(x=>x.name));const c=KitStore.clone(p);c.name=name;
 k.panels.splice(k.panels.indexOf(p)+1,0,c);
 for(const s of k.binSets)if(s.panels[p.name])s.panels[name]=KitStore.clone(s.panels[p.name]);
 if(k.stutter?.panels?.[p.name])k.stutter.panels[name]=KitStore.clone(k.stutter.panels[p.name]);
 ui.sel={type:'kit',kit:k.name};ui.selRow=k.panels.indexOf(c);ui.editCell={row:ui.selRow,col:0};touch();render()}
// New Marker adds a row to the panel's marker table; each cell is then edited in place (help: Creating Markers).
function newMarker(){const k=selKit(),p=selPanel();if(!p)return message(T('selectPanelFirst'));
 const name=uniqueName('New Marker',p.markers.map(m=>m.name));
 const last=p.markers[p.markers.length-1];const lo=last?+(last.max_bp+5).toFixed(2):100;
 p.markers.push({name,dye:last?last.dye:'blue',min_bp:lo,max_bp:+(lo+40).toFixed(2),control_alleles:[],repeat:4,option:'none',y_marker:false,ladder_alleles:[],comment:''});
 ui.sel={type:'panel',kit:k.name,panel:p.name};ui.selRow=p.markers.length-1;ui.editCell={row:ui.selRow,col:0};touch();render()}
function newBinSet(){const k=selKit();if(!k)return message(T('selectKitFirst'));
 askName(T('newBinSetTitle'),T('binSetName'),'',v=>{if(k.binSets.some(s=>s.name===v)){message(fmt('binSetExists',v));return false}
  k.binSets.push({name:v,version:'GMID-X v 1.2',panels:{}});ui.binSet=v;touch();render()})}
function newItem(){const s=ui.sel.type;if(s==='root')newKit();else if(s==='kit')newPanel();else newMarker()}
function deleteNode(){
 const s=ui.sel.type==='stutter'?{...ui.sel,type:'marker'}:ui.sel,k=selKit();
 if(s.type==='kit')confirmBox(T('confirmDeleteKit'),()=>{work.kits=work.kits.filter(x=>x!==k);ui.sel={type:'root'};touch();render()});
 else if(s.type==='panel')confirmBox(T('confirmDeletePanel'),()=>{k.panels=k.panels.filter(x=>x.name!==s.panel);
  for(const set of k.binSets)delete set.panels[s.panel];if(k.stutter)delete k.stutter.panels[s.panel];ui.sel={type:'kit',kit:k.name};touch();render()});
 else if(s.type==='marker')confirmBox(T('confirmDeleteMarker'),()=>{const p=selPanel();p.markers=p.markers.filter(x=>x.name!==s.marker);
  for(const set of k.binSets)if(set.panels[p.name])delete set.panels[p.name][s.marker];
  if(k.stutter?.panels?.[p.name])delete k.stutter.panels[p.name][s.marker];ui.sel={type:'panel',kit:k.name,panel:p.name};touch();render()});
 else message(T('selectNode'))}
function deleteBinSet(){const k=selKit(),set=curSet();if(!set)return message(T('selectBinSetFirst'));
 confirmBox(T('cannotUndo'),()=>{k.binSets=k.binSets.filter(x=>x!==set);ui.binSet=null;touch();render()})}

// ---------- bins ----------
function binDialog(existing,at){
 const k=selKit(),p=selPanel(),m=selMarker(),set=curSet();
 if(!m)return message(T('selectMarkerFirst'));if(!set)return message(T('selectBinSetFirst'));
 const bins=set.panels[p.name]?.[m.name]||[];
 const b0=existing||{label:'',center_bp:+((at!=null?at:(m.min_bp+m.max_bp)/2).toFixed(2)),left_bp:0.5,right_bp:0.5,virtual:false};
 const locked=existing&&(m.ladder_alleles.includes(existing.label)||m.control_alleles.includes(existing.label));
 dialog(existing?T('binProperties'):T('addBinProperties'),b=>{
  const f=(id,label,v,attrs={})=>el('div',{class:'field'},el('label',{text:label,for:id}),el('input',{type:'text',id,value:String(v),...attrs}));
  b.append(f('bnName',T('binName'),b0.label,locked?{disabled:''}:{}),f('bnLoc',T('location'),b0.center_bp),f('bnLeft',T('leftOffset'),b0.left_bp),
   f('bnRight',T('rightOffset'),b0.right_bp),el('label',{class:'chk'},el('input',{type:'checkbox',id:'bnVirt',...(b0.virtual?{checked:''}:{})}),document.createTextNode(' '+T('virtualBinChk'))));
  if(locked)b.append(el('div',{class:'note',text:fmt(m.ladder_alleles.includes(existing.label)?'ladderNoRename':'controlNoRename',existing.label)}))},
  [[T('ok'),body=>{
   const v=id=>body.querySelector('#'+id).value.trim();
   const label=locked?existing.label:v('bnName');const c=+v('bnLoc'),l=+v('bnLeft'),r=+v('bnRight');
   if(!label||!/^[^\t,]{1,255}$/.test(label)){message(T('badBinName'));return false}
   if(!isFinite(c)||v('bnLoc')===''){message(T('badLocation'));return false}
   if(!isFinite(l)||l<0||v('bnLeft')===''){message(T('badLeft'));return false}
   if(!isFinite(r)||r<0||v('bnRight')===''){message(T('badRight'));return false}
   if(c-l<m.min_bp||c+r>m.max_bp){message(T('binOutside'));return false}
   const others=bins.filter(x=>x!==existing);
   if(others.some(x=>x.label.toLowerCase()===label.toLowerCase())){message(T('dupBinName'));return false}
   if(others.some(x=>Math.max(x.center_bp-x.left_bp,c-l)<=Math.min(x.center_bp+x.right_bp,c+r))){message(T('binOverlap'));return false}
   const nb={label,center_bp:c,left_bp:l,right_bp:r,virtual:body.querySelector('#bnVirt').checked};
   set.panels[p.name]=set.panels[p.name]||{};
   const list=others.concat([nb]).sort((a,b)=>a.center_bp-b.center_bp);set.panels[p.name][m.name]=list;
   if(existing&&existing.label!==label)m.ladder_alleles=m.ladder_alleles.map(x=>x===existing.label?label:x);
   ui.selBin=label;touch();render()},true],[T('cancel'),()=>true]])}
function editBin(){const m=selMarker();const bins=markerBins(curSet(),ui.sel.panel,ui.sel.marker)||[];const b=bins.find(x=>x.label===ui.selBin);
 if(!m)return message(T('selectMarkerFirst'));if(!b)return message(T('selectBin'));binDialog(b)}
function deleteBin(){const set=curSet(),m=selMarker();if(!m)return message(T('selectMarkerFirst'));
 const bins=markerBins(set,ui.sel.panel,m.name)||[];const b=bins.find(x=>x.label===ui.selBin);if(!b)return message(T('selectBin'));
 set.panels[ui.sel.panel][m.name]=bins.filter(x=>x!==b);m.ladder_alleles=m.ladder_alleles.filter(x=>x!==b.label);ui.selBin=null;touch();render()}

// ---------- stutter ----------
function stutterDialog(dir,index){
 const k=selKit(),p=selPanel(),m=selMarker();if(!m)return message(T('selectMarkerFirst'));
 const all=stutterOf(k,p.name,m.name);const mine=all.filter(s=>s.direction===dir);
 const cur=index!=null?mine[index]:null;
 if(!cur&&mine.length>=4)return message(T('max4'));
 dialog(T('stutterDialog'),b=>{
  const f=(id,label,v)=>el('div',{class:'field'},el('label',{text:label,for:id}),el('input',{type:'text',id,value:String(v)}));
  const sel=el('select',{id:'stDir'},new Option(T('minusStutter'),'minus'),new Option(T('plusStutter'),'plus'));sel.value=dir;
  b.append(el('div',{class:'field'},el('label',{text:T('typeOfStutter')}),sel),f('stRatio',T('ratioBetween'),cur?cur.ratio:''),
   f('stFrom',T('fromDistance'),cur?cur.min_bp:3.25),f('stTo',T('toDistance'),cur?cur.max_bp:4.75))},
  [[T('ok'),body=>{const v=id=>body.querySelector('#'+id).value.trim();
   const ratio=+v('stRatio'),lo=+v('stFrom'),hi=+v('stTo'),d=v('stDir');
   if(v('stRatio')===''||!isFinite(ratio)){message(T('ratioNumeric'));return false}
   if(ratio<0||ratio>1){message(T('ratioRange'));return false}
   if(!isFinite(lo)||!isFinite(hi)||v('stFrom')===''||v('stTo')===''){message(T('distNumeric'));return false}
   if(lo>=hi){message(T('fromLtTo'));return false}
   const rest=all.filter(s=>s!==cur);
   if(rest.filter(s=>s.direction===d).length>=4){message(T('max4'));return false}
   if(rest.some(s=>s.direction===d&&Math.max(s.min_bp,lo)<Math.min(s.max_bp,hi))){message(T('distOverlap'));return false}
   k.stutter=k.stutter||{version:'SSFv1.2',panels:{}};k.stutter.panels[p.name]=k.stutter.panels[p.name]||{};
   k.stutter.panels[p.name][m.name]=rest.concat([{ratio,min_bp:lo,max_bp:hi,direction:d}]);touch();render()},true],[T('cancel'),()=>true]])}
function deleteStutter(dir,index){const k=selKit(),p=selPanel(),m=selMarker();const all=stutterOf(k,p.name,m.name);
 const mine=all.filter(s=>s.direction===dir);if(index==null||!mine[index])return message(T('selectRow'));
 k.stutter.panels[p.name][m.name]=all.filter(s=>s!==mine[index]);touch();render()}

// ---------- navigator ----------
function node(iconName,text,sel,onClick,kids){
 const li=el('li');const n=el('span',{class:'tnode'+(sel?' sel':'')});n.innerHTML=icon(iconName);n.append(document.createTextNode(text));
 n.addEventListener('click',onClick);li.append(n);if(kids&&kids.length){const ul=el('ul',{class:'tree'});ul.append(...kids);li.append(ul)}return li}
function renderNavigator(){
 const s=ui.sel;const nav=$('navigator');nav.classList.toggle('hidden',!!ui.hideNav);nav.replaceChildren();
 const root=el('ul',{class:'tree root'});
 const kits=visKits().map(k=>node('kit',k.name,s.type==='kit'&&s.kit===k.name,()=>go({type:'kit',kit:k.name}),
  (s.kit===k.name||ui.expanded.has(k.name))?k.panels.map(p=>node('panel',p.name,s.type==='panel'&&s.kit===k.name&&s.panel===p.name,()=>go({type:'panel',kit:k.name,panel:p.name}),
   (s.kit===k.name&&s.panel===p.name)?p.markers.map(m=>node('marker',m.name,s.type==='marker'&&s.marker===m.name,()=>go({type:'marker',kit:k.name,panel:p.name,marker:m.name}),
    s.marker===m.name?[node('stutter',T('stutterNode'),s.type==='stutter',()=>go({type:'stutter',kit:k.name,panel:p.name,marker:m.name}))]:[])):[])):[]));
 root.append(node('panelManager',T('panelMgrTitle'),s.type==='root',()=>go({type:'root'}),kits));nav.append(root);
 nav.querySelector('.tnode.sel')?.scrollIntoView({block:'nearest'})}
function go(sel){if(sel.kit&&sel.kit!==ui.sel.kit)ui.binSet=null;ui.sel=sel;ui.selBin=null;ui.selRow=null;if(sel.kit)ui.expanded.add(sel.kit);ui.view=null;render()}

// ---------- tables ----------
// cols: [header, width, editor?]; editor = {type:'text'|'select'|'check', options, value(i), set(i,v) -> error text or ''}.
// Click a cell of the selected row (or type over a new row) to edit it; Enter or leaving the cell commits, Esc cancels.
function table(cols,rows,onSel,selIndex,opts={}){
 const t=el('table',{class:'g'+(opts.editable?' editable':'')});const cg=el('colgroup');cg.append(el('col',{style:'width:28px'}));for(const [,w] of cols)cg.append(el('col',{style:`width:${w}px`}));t.append(cg);
 const hr=el('tr');hr.append(el('th',{text:'',style:'width:28px'}));for(const [h] of cols)hr.append(el('th',{text:h,title:h}));t.append(el('thead',{},hr));
 const tb=el('tbody');let cur=selIndex;
 const select=i=>{cur=i;tb.querySelectorAll('tr').forEach((tr,j)=>tr.classList.toggle('sel',j===i));if(onSel)onSel(i)};
 rows.forEach((r,i)=>{const tr=el('tr',{class:i===selIndex?'sel':''});tr.append(el('td',{class:'rh',text:String(i+1)}));
  r.forEach((c,ci)=>{const td=el('td');const ed=cols[ci][2];
   if(ed&&ed.type==='check'){const cb=el('input',{type:'checkbox'});cb.checked=!!ed.value(i);
    cb.addEventListener('change',()=>{const err=ed.set(i,cb.checked);if(err){message(err);cb.checked=!cb.checked}});td.append(cb);td.classList.add('c')}
   else if(c instanceof Node)td.append(c);else{td.textContent=c==null?'':String(c);td.title=td.textContent}
   if(ed&&ed.type!=='check'){td.classList.add('ed');td.addEventListener('click',()=>{if(cur===i&&!td.querySelector('input,select'))startEdit(td,ed,i)})}
   tr.append(td)});
  tr.addEventListener('mousedown',e=>{if(cur!==i)select(i)});
  if(opts.open)tr.firstChild.addEventListener('dblclick',()=>opts.open(i));
  tb.append(tr)});
 t.append(tb);
 const ec=ui.editCell;if(ec&&opts.editable){ui.editCell=null;const td=tb.children[ec.row]?.children[ec.col+1];const ed=cols[ec.col]?.[2];
  if(td&&ed)setTimeout(()=>startEdit(td,ed,ec.row),0)}
 return t}
function startEdit(td,ed,i){
 const old=ed.value(i);let input;
 if(ed.type==='select'){input=el('select');for(const [v,l] of ed.options)input.append(new Option(l,v));input.value=String(old)}
 else input=el('input',{type:'text',value:old==null?'':String(old)});
 td.replaceChildren(input);input.focus();if(input.select)input.select();
 let done=false;
 const finish=commit=>{if(done)return;done=true;
  if(commit&&String(input.value)!==String(old)){const err=ed.set(i,input.value.trim());if(err)message(err);else touch()}
  render()};
 input.addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();finish(true)}else if(e.key==='Escape'){e.preventDefault();finish(false)}e.stopPropagation()});
 input.addEventListener('blur',()=>setTimeout(()=>finish(true),0));
 if(ed.type==='select')input.addEventListener('change',()=>finish(true))}
const split=s=>String(s).split(',').map(x=>x.trim()).filter(Boolean);
function renameKit(k,name){
 if(!name||!NAME_RE.test(name))return T('invalidChars');if(name!==k.name&&kitOf(name))return fmt('kitExists',name);
 ui.expanded.delete(k.name);if(ui.sel.kit===k.name)ui.sel.kit=name;k.name=name;ui.expanded.add(name);return ''}
function renamePanel(k,p,name){
 if(!name||!NAME_RE.test(name))return T('invalidChars');if(name===p.name)return '';if(k.panels.some(x=>x.name===name))return fmt('nameUsed',name);
 for(const s of k.binSets)if(s.panels[p.name]){s.panels[name]=s.panels[p.name];delete s.panels[p.name]}
 const sp=k.stutter?.panels;if(sp&&sp[p.name]){sp[name]=sp[p.name];delete sp[p.name]}
 if(ui.sel.panel===p.name)ui.sel.panel=name;p.name=name;return ''}
// Same checks as the Edit Marker dialog, one field at a time.
function setMarkerField(k,p,m,field,v){
 const bins=()=>k.binSets.flatMap(s=>s.panels[p.name]?.[m.name]||[]);
 if(field==='name'){if(!v||!NAME_RE.test(v))return T('invalidChars');if(v===m.name)return '';if(p.markers.some(x=>x.name===v))return T('markerExists');
  for(const s of k.binSets){const pb=s.panels[p.name];if(pb&&pb[m.name]){pb[v]=pb[m.name];delete pb[m.name]}}
  const sp=k.stutter?.panels?.[p.name];if(sp&&sp[m.name]){sp[v]=sp[m.name];delete sp[m.name]}
  if(ui.sel.marker===m.name)ui.sel.marker=v;m.name=v;return ''}
 if(field==='min_bp'||field==='max_bp'){const n=+v;if(v===''||!isFinite(n))return T('numeric');
  const lo=field==='min_bp'?n:m.min_bp,hi=field==='max_bp'?n:m.max_bp;if(lo<0)return T('minZero');if(hi<=lo)return T('maxGtMin');
  if(bins().some(b=>b.center_bp-b.left_bp<lo||b.center_bp+b.right_bp>hi))return T('binsOutsideRange');m[field]=n;return ''}
 if(field==='dye'){m.dye=v;return ''}
 if(field==='repeat'){m.repeat=+v;return ''}
 if(field==='control_alleles'||field==='ladder_alleles'){m[field]=/^none$/i.test(v)?[]:split(v);return ''}
 if(field==='y_marker'){m.y_marker=!!v;return ''}
 if(field==='internal_qc'){if(v)m.internal_qc=true;else delete m.internal_qc;return ''}
 if(field==='comment'){m.comment=v;return ''}
 return ''}
function dyeCell(d){const s=el('span',{class:'dye'});s.append(el('i',{style:`background:${DYE_CSS[d]||'#888'}`}),document.createTextNode(T('dye_'+d)));return s}
function stutterSummary(k,p,m){const st=stutterOf(k,p,m).filter(s=>s.direction==='minus');return st.length?st.map(s=>s.ratio).join(', '):''}
function markerTable(k,p,only){
 const list=only?[only]:p.markers;
 const E=(field,type,options)=>({type,options,value:i=>{const m=list[i];return field.endsWith('alleles')?m[field].join(field==='ladder_alleles'?', ':','):m[field]},
  set:(i,v)=>{const e=setMarkerField(k,p,list[i],field,v);if(!e&&type==='check'){touch();setTimeout(render,0)}return e}});
 const cols=[[T('markerName'),120,E('name','text')],[T('dyeColor'),80,E('dye','select',DYES.map(d=>[d,T('dye_'+d)]))],[T('minSize'),70,E('min_bp','text')],[T('maxSize'),70,E('max_bp','text')],
  [T('controlAlleles'),110,E('control_alleles','text')],[T('markerRepeat'),60,E('repeat','select',REPEATS.map(r=>[String(r),String(r)]))],
  [T('stutterPct'),110],[T('ladderAlleles'),330,E('ladder_alleles','text')],[T('yMarker'),60,E('y_marker','check')],[T('internalQc'),70,E('internal_qc','check')],[T('comments'),110,E('comment','text')]];
 const rows=list.map(m=>[m.name,dyeCell(m.dye),m.min_bp,m.max_bp,m.control_alleles.join(','),m.repeat,stutterSummary(k,p.name,m.name),
  m.ladder_alleles.join(', '),'','',m.comment||'']);
 return table(cols,rows,i=>{ui.selRow=i},only?0:ui.selRow,{editable:true,open:i=>only?editMarker(k,p,list[i]):go({type:'marker',kit:k.name,panel:p.name,marker:list[i].name})})}
function editMarker(k,p,m){
 dialog(fmt('editMarkerTitle',m.name),b=>{
  const f=(id,label,v)=>el('div',{class:'field'},el('label',{text:label,for:id}),el('input',{type:'text',id,value:String(v)}));
  const dye=el('select',{id:'mkDye'});for(const d of DYES)dye.append(new Option(T('dye_'+d),d));dye.value=m.dye;
  const rep=el('select',{id:'mkRep'});for(const r of REPEATS)rep.append(new Option(String(r),String(r)));rep.value=String(m.repeat);
  b.append(f('mkName',T('markerName'),m.name),el('div',{class:'field'},el('label',{text:T('dyeColor')}),dye),f('mkMin',T('minSize'),m.min_bp),f('mkMax',T('maxSize'),m.max_bp),
   f('mkCtl',T('controlAlleles'),m.control_alleles.join(',')),el('div',{class:'field'},el('label',{text:T('markerRepeat')}),rep),
   f('mkLad',T('ladderAlleles'),m.ladder_alleles.join(', ')),
   el('label',{class:'chk'},el('input',{type:'checkbox',id:'mkY',...(m.y_marker?{checked:''}:{})}),document.createTextNode(' '+T('yMarker'))),
   f('mkCom',T('comments'),m.comment||''))},
  [[T('ok'),body=>{const v=id=>body.querySelector('#'+id).value.trim();
   const name=v('mkName'),lo=+v('mkMin'),hi=+v('mkMax');
   if(!name||!NAME_RE.test(name)){message(T('invalidChars'));return false}
   if(name!==m.name&&p.markers.some(x=>x.name===name)){message(T('markerExists'));return false}
   if(!isFinite(lo)||!isFinite(hi)||v('mkMin')===''||v('mkMax')===''){message(T('numeric'));return false}
   if(lo<0){message(T('minZero'));return false}
   if(hi<=lo){message(T('maxGtMin'));return false}
   const bins=k.binSets.flatMap(s=>s.panels[p.name]?.[m.name]||[]);
   if(bins.some(b=>b.center_bp-b.left_bp<lo||b.center_bp+b.right_bp>hi)){message(T('binsOutsideRange'));return false}
   const split=s=>s.split(',').map(x=>x.trim()).filter(Boolean);
   Object.assign(m,{dye:body.querySelector('#mkDye').value,min_bp:lo,max_bp:hi,control_alleles:split(v('mkCtl')),repeat:+body.querySelector('#mkRep').value,
    ladder_alleles:split(v('mkLad')),y_marker:body.querySelector('#mkY').checked,comment:v('mkCom')});
   if(name!==m.name){for(const s of k.binSets){const pb=s.panels[p.name];if(pb&&pb[m.name]){pb[name]=pb[m.name];delete pb[m.name]}}
    const sp=k.stutter?.panels?.[p.name];if(sp&&sp[m.name]){sp[name]=sp[m.name];delete sp[m.name]}
    if(ui.sel.marker===m.name)ui.sel.marker=name;m.name=name}
   touch();render()},true],[T('cancel'),()=>true]])}

// ---------- plots (SVG) ----------
const SVGNS='http://www.w3.org/2000/svg';
const sv=(tag,attrs={},text)=>{const n=document.createElementNS(SVGNS,tag);for(const [k,v] of Object.entries(attrs))n.setAttribute(k,v);if(text!=null)n.textContent=text;return n};
function axis(svg,x0,x1,X,top,W){
 const span=x1-x0;const step=span>200?50:span>80?20:span>30?10:span>12?5:1;
 for(let v=Math.ceil(x0/step)*step;v<=x1;v+=step){const x=X(v);svg.append(sv('line',{x1:x,x2:x,y1:top,y2:top+5,stroke:'#555'}),sv('text',{x,y:top-2,'text-anchor':'middle','font-size':10,fill:'#333'},String(v)))}
 svg.append(sv('line',{x1:0,x2:W,y1:top+5,y2:top+5,stroke:'#555'}))}
function panelPlot(k,p){
 const box=el('div',{class:'pmplot'});const W=Math.max(600,($('pmContent').clientWidth||900)-20);
 const dyes=DYES.filter(d=>p.markers.some(m=>m.dye===d));const H=30+dyes.length*28;
 const lo=Math.min(...p.markers.map(m=>m.min_bp),60)-10,hi=Math.max(...p.markers.map(m=>m.max_bp),120)+10;const X=v=>(v-lo)/(hi-lo)*W;
 const svg=sv('svg',{width:W,height:H,class:'binsvg'});axis(svg,lo,hi,X,14,W);
 dyes.forEach((d,i)=>{const y=26+i*28;for(const m of p.markers.filter(x=>x.dye===d)){
  const r=sv('rect',{x:X(m.min_bp),y,width:Math.max(2,X(m.max_bp)-X(m.min_bp)),height:18,fill:DYE_CSS[d],opacity:.8,rx:2,class:'mk'});
  r.append(sv('title',{},`${m.name} (${m.min_bp}–${m.max_bp})`));r.addEventListener('dblclick',()=>go({type:'marker',kit:k.name,panel:p.name,marker:m.name}));
  svg.append(r,sv('text',{x:(X(m.min_bp)+X(m.max_bp))/2,y:y+13,'text-anchor':'middle','font-size':10,fill:'#fff','pointer-events':'none'},m.name))}});
 box.append(svg);return box}
function binPlot(k,p,m){
 const set=curSet();const bins=markerBins(set,p.name,m.name);
 const box=el('div',{class:'pmplot'});
 if(!set){box.append(el('div',{class:'empty',text:T('createBinSetFirstView')}));return box}
 const W=Math.max(600,($('pmContent').clientWidth||900)-20),H=150;
 const full=[m.min_bp-2,m.max_bp+2];const [lo,hi]=ui.view||full;const X=v=>(v-lo)/(hi-lo)*W;
 const svg=sv('svg',{width:W,height:H,class:'binsvg'});
 svg.append(sv('rect',{x:X(m.min_bp),y:20,width:X(m.max_bp)-X(m.min_bp),height:8,fill:DYE_CSS[m.dye]||'#888'}));
 axis(svg,lo,hi,X,12,W);
 for(const b of bins||[]){const x=X(b.center_bp-b.left_bp),w=Math.max(1,X(b.center_bp+b.right_bp)-x);
  const sel=b.label===ui.selBin;
  const g=sv('g',{class:'bin'+(sel?' sel':'')});
  g.append(sv('rect',{x,y:36,width:w,height:H-62,fill:b.virtual?'#f6c6d8':'#cfcfcf',stroke:sel?'#0a246a':'#8a8a8a','stroke-width':sel?2:1}),
   sv('text',{x:X(b.center_bp),y:H-14,'text-anchor':'middle','font-size':10,fill:m.ladder_alleles.includes(b.label)?'#000':'#666'},b.label),
   sv('title',{},`${b.label}: ${b.center_bp} (−${b.left_bp}/+${b.right_bp})${b.virtual?' '+T('virtualBin'):''}`));
  g.addEventListener('mousedown',e=>{e.stopPropagation();ui.selBin=b.label;render()});
  g.addEventListener('dblclick',e=>{e.stopPropagation();binDialog(b)});
  g.addEventListener('contextmenu',e=>{e.preventDefault();e.stopPropagation();ui.selBin=b.label;render();
   ctxMenu(e.clientX,e.clientY,[{label:T('addBin'),run:()=>binDialog(null,bpAt(e))},{label:T('editBin'),run:()=>binDialog(b)},{label:T('deleteBin'),run:deleteBin}])});
  svg.append(g)}
 if(!bins||!bins.length)svg.append(sv('text',{x:W/2,y:H/2,'text-anchor':'middle',fill:'#777','font-size':12},T('markerNoBins')));
 // zoom: drag on the ruler; wheel zooms around the pointer
 let start=null;const band=sv('rect',{y:0,height:H,fill:'rgba(10,36,106,.15)',visibility:'hidden'});svg.append(band);
 const bp=e=>lo+(e.offsetX/W)*(hi-lo);
 const bpAt=e=>{const r=svg.getBoundingClientRect();const v=lo+((e.clientX-r.left)/W)*(hi-lo);return Math.min(m.max_bp-0.5,Math.max(m.min_bp+0.5,v))};
 svg.addEventListener('contextmenu',e=>{e.preventDefault();ctxMenu(e.clientX,e.clientY,[{label:T('addBin'),run:()=>binDialog(null,bpAt(e))},{label:T('editBin'),disabled:!ui.selBin,run:editBin},{label:T('deleteBin'),disabled:!ui.selBin,run:deleteBin}])});
 svg.addEventListener('mousedown',e=>{if(e.offsetY<34){start=e.offsetX;band.setAttribute('x',start);band.setAttribute('width',0);band.setAttribute('visibility','visible')}
  else{ui.selBin=null;render()}});
 svg.addEventListener('mousemove',e=>{$('xyCell').textContent=fmt('locationBp',bp(e).toFixed(2));
  if(start!=null){band.setAttribute('x',Math.min(start,e.offsetX));band.setAttribute('width',Math.abs(e.offsetX-start))}});
 svg.addEventListener('mouseup',e=>{if(start==null)return;const a=lo+start/W*(hi-lo),b=bp(e);start=null;band.setAttribute('visibility','hidden');
  if(Math.abs(b-a)>0.3){ui.view=[Math.min(a,b),Math.max(a,b)];render()}});
 svg.addEventListener('wheel',e=>{e.preventDefault();const c=bp(e),f=e.deltaY>0?1.25:0.8;let a=c-(c-lo)*f,b=c+(hi-c)*f;
  if(b-a>=full[1]-full[0]){ui.view=null}else ui.view=[a,b];render()},{passive:false});
 svg.addEventListener('dblclick',e=>{if(e.offsetY<34){ui.view=null;render()}});
 box.append(svg);return box}
function stutterTables(k,p,m){
 const wrap=el('div',{class:'pmstutter'});wrap.append(el('div',{class:'cap',text:T('stutterTitle')}));
 const st=stutterOf(k,p.name,m.name);
 for(const dir of ['minus','plus']){const mine=st.filter(s=>s.direction===dir);
  const sec=el('fieldset',{},el('legend',{text:T(dir==='minus'?'minusStutter':'plusStutter')}));
  let sel=null;
  const t=table([[T('stutterRatio'),80],[T('fromDistance'),110],[T('toDistance'),110]],mine.map(s=>[s.ratio,s.min_bp,s.max_bp]),i=>{sel=i;
   t.querySelectorAll('tbody tr').forEach((tr,j)=>tr.classList.toggle('sel',j===i))});
  t.querySelectorAll('tbody tr').forEach((tr,i)=>tr.addEventListener('dblclick',()=>stutterDialog(dir,i)));
  const bar=el('div',{class:'btnrow'});
  const btn=(label,fn)=>{const b=el('button',{class:'btn',type:'button',text:label});b.addEventListener('click',fn);return b};
  bar.append(btn(T('newBtn'),()=>stutterDialog(dir,null)),btn(T('editBtn'),()=>sel==null?message(T('selectRowEdit')):stutterDialog(dir,sel)),
   btn(T('deleteBtn'),()=>sel==null?message(T('selectRowDelete')):deleteStutter(dir,sel)));
  sec.append(t,bar);wrap.append(sec)}
 if(!st.length)wrap.append(el('div',{class:'note',text:fmt('stutterGlobalNote',m.name)}));
 return wrap}

// ---------- content ----------
function renderContent(){
 const c=$('pmContent');c.replaceChildren();const s=ui.sel;const k=selKit(),p=selPanel(),m=selMarker();
 if(s.type==='root'){
  const vk=visKits();
  const E=(f,type,options)=>({type,options,value:i=>vk[i][f]||'',set:(i,v)=>{const x=vk[i];if(f==='name')return renameKit(x,v);x[f]=v;return ''}});
  c.append(el('div',{class:'grid pmgrid'},table([[T('kitName'),260,E('name','text')],[T('comments'),260,E('comment','text')],[T('kitType'),140],[T('securityGroupLbl'),220,E('security_group','select',groups().map(g=>[g,g]))]],
   vk.map(x=>[x.name,x.comment||'','MICROSATELLITE',x.security_group||groups()[0]]),i=>{ui.selRow=i},ui.selRow,{editable:true,open:i=>go({type:'kit',kit:vk[i].name})})));
  if(!vk.length)c.append(el('div',{class:'empty',text:T('panelHint')}));
 }else if(s.type==='kit'&&k){
  const E=f=>({type:'text',value:i=>k.panels[i][f]||'',set:(i,v)=>f==='name'?renamePanel(k,k.panels[i],v):(k.panels[i][f]=v,'')});
  c.append(el('div',{class:'grid pmgrid'},table([[T('panelName'),260,E('name')],[T('comments'),260,E('comment')],[T('markersCol'),80],[T('binSets'),260]],
   k.panels.map(x=>[x.name,x.comment||'',x.markers.length,k.binSets.filter(b=>b.panels[x.name]).map(b=>b.name).join(', ')]),i=>{ui.selRow=i},ui.selRow,
   {editable:true,open:i=>go({type:'panel',kit:k.name,panel:k.panels[i].name})})));
  if(!k.binSets.length)c.append(el('div',{class:'note',text:T('importBinsHint')}));
 }else if(s.type==='panel'&&p){
  if(p.markers.length)c.append(panelPlot(k,p));
  c.append(el('div',{class:'grid pmgrid'},markerTable(k,p)));
  if(!p.markers.length)c.append(el('div',{class:'note',text:T('newMarkerHint')}));
 }else if(s.type==='marker'&&m){
  c.append(binPlot(k,p,m),el('div',{class:'grid pmgrid small'},markerTable(k,p,m)));
 }else if(s.type==='stutter'&&m){
  c.append(el('div',{class:'grid pmgrid small'},markerTable(k,p,m)),stutterTables(k,p,m));
 }}
function renderBinSetCombo(){
 const combo=$('binSetCombo');combo.replaceChildren();const k=selKit();
 for(const s of k?.binSets||[])combo.append(new Option(s.name,s.name));
 combo.disabled=!k||!k.binSets.length;const set=curSet();if(set){combo.value=set.name;ui.binSet=set.name}}
function updateToolbar(){const s=ui.sel.type,has=!!selMarker()&&!!curSet();
 const set=(id,on)=>{const b=$(id);if(b)b.disabled=!on};
 const nb=$('tbNew');if(nb)nb.title=T(s==='root'?'newKit':s==='kit'?'newPanel':'newMarker');
 set('tbNew',true);set('tbClear',s!=='root');set('tbAddBin',has&&s==='marker');set('tbDeleteBin',has&&!!ui.selBin);set('tbEditBin',has&&!!ui.selBin);set('tbNewBinSet',!!selKit())}
function render(){renderNavigator();renderBinSetCombo();renderContent();updateToolbar();
 const s=ui.sel;$('markerInfo').textContent=(s.type==='marker'||s.type==='stutter')?`${s.kit} › ${s.panel} › ${s.marker}`:s.type==='panel'?`${s.kit} › ${s.panel}`:s.kit||'';
 $('applyBtn').disabled=!ui.dirty}

// ---------- commit ----------
async function apply(){
 if(window.Sec&&!Sec.can('UPDATE_PANEL')){Sec.denied();return false}
 for(const k of work.kits){if(!k.security_group)k.security_group=window.Sec&&Sec.defaultGroup?Sec.defaultGroup():'GeneMapper ID-X Security Group';if(k.owner===undefined)k.owner=me()}
 {const old=new Map(saved.kits.map(k=>[k.name,k]));const bad=[];
  for(const k of work.kits){const o=old.get(k.name);
   if(!o){if(!kitCan(k,'create'))bad.push([k.name,k.security_group,'create'])}
   else if(JSON.stringify(o)!==JSON.stringify(k)){if(!kitCan(o,'update'))bad.push([k.name,o.security_group,'update']);
    if((o.security_group||'GeneMapper ID-X Security Group')!==k.security_group&&!kitCan(o,'control'))bad.push([k.name,o.security_group,'control'])}}
  for(const o of saved.kits)if(!kitOf(o.name)&&!kitCan(o,'delete'))bad.push([o.name,o.security_group,'delete']);
  if(bad.length){message(bad.map(([n,g,op])=>fmt('scItemDenied',n,g||'GeneMapper ID-X Security Group',T('scOp_'+op))).join('\n'));return false}}
 const before=new Map(saved.kits.map(k=>[k.name,JSON.stringify(k)])),after=new Map(work.kits.map(k=>[k.name,JSON.stringify(k)]));const items=[];
 for(const [n,v] of after)if(!before.has(n))items.push({map:'Kit',attribute:'created',instance:n,old:'',new:n,event:'Add Chemistry Kit'});else if(before.get(n)!==v)items.push({map:'Kit',attribute:'modified',instance:n,old:n,new:n,event:'Modify Chemistry Kit'});
 for(const n of before.keys())if(!after.has(n))items.push({map:'Kit',attribute:'deleted',instance:n,old:n,new:'',event:'Delete Chemistry Kit'});
 let ch={commit(){}};if(window.Sec&&items.length){const A={created:'CREATE_CHEMISTRY_KIT',modified:'MODIFY_CHEMISTRY_KIT',deleted:'DELETE_CHEMISTRY_KIT'}[items[0].attribute];
  ch=await Sec.change({items,esig:{group:'GM_CHEMISTRY_KIT',action:A}});if(!ch)return false}
 if(!KitStore.save(work)){message(T('storageFull'));return false}ch.commit();saved=KitStore.clone(work);ui.dirty=false;render();status(T('applied'));
 try{window.opener&&window.opener.OT&&window.opener.OT.kitsChanged&&window.opener.OT.kitsChanged()}catch(e){}return true}
function closeWin(){if(ui.dirty)confirmBox(T('confirmClose'),()=>{ui.dirty=false;window.close()});else window.close()}

function buildChrome(){
 document.documentElement.lang=I18N.lang;document.title=T('panelMgrTitle')+' — OpenTrace';
 buildMenubar($('menubar'),[
  {label:T('file'),items:[{label:T('newKit'),run:newKit},{label:T('duplicateKit'),run:duplicateKit},{label:T('newPanel'),run:newPanel},{label:T('duplicatePanel'),run:duplicatePanel},{label:T('newMarker'),run:newMarker},'-',{label:T('importPanels'),run:importPanels},{label:T('importBinSet'),run:importBins},{label:T('importStutter'),run:importStutter},'-',
   {label:T('exportPanels'),run:exportPanels},{label:T('exportBinSet'),run:exportBinSet},{label:T('exportStutter'),run:exportStutter},{label:T('exportAllKits'),run:exportAll},'-',
   {label:T('print'),run:()=>window.print()},'-',{label:T('closeWin'),run:closeWin}]},
  {label:T('edit'),items:[{label:T('undo'),disabled:true},'-',{label:T('deleteItem'),run:deleteNode}]},
  {label:T('binsMenu'),items:[{label:T('newBinSet'),run:newBinSet},{label:T('deleteBinSet'),run:deleteBinSet},'-',{label:T('addBin'),run:()=>binDialog(null)},
   {label:T('editBin'),run:editBin},{label:T('deleteBin'),run:deleteBin},'-',{label:T('autoBin'),disabled:true},{label:T('addReferenceData'),disabled:true}]},
  {label:T('view'),items:[{label:T('showNavigator'),checked:!ui.hideNav,run:()=>{ui.hideNav=!ui.hideNav;buildChrome();render()}},{label:T('fullView'),run:()=>{ui.view=null;render()}},'-',
   {label:T('language'),sub:[{label:'Русский',checked:I18N.lang==='ru',run:()=>{I18N.set('ru');buildChrome();render()}},{label:'English',checked:I18N.lang==='en',run:()=>{I18N.set('en');buildChrome();render()}}]}]},
  {label:T('help'),items:[{label:T('about'),run:()=>message(T('panelHint'),T('panelMgrTitle'))}]}]);
 const tb=$('toolbar');tb.replaceChildren(
  tbutton('newItem',T('newKit'),newItem,{id:'tbNew'}),tbutton('clearItem',T('clearBtn'),deleteNode,{id:'tbClear'}),tsep(),
  tbutton('addBin',T('addBin'),()=>binDialog(null),{id:'tbAddBin'}),tbutton('deleteBin',T('deleteBin'),deleteBin,{id:'tbDeleteBin'}),tbutton('editBin',T('editBin'),editBin,{id:'tbEditBin'}),
  tbutton('fullView',T('fullView'),()=>{ui.view=null;render()}),tsep(),
  tbutton('panel',T('importPanels'),importPanels,{id:'tbImportPanels'}),tbutton('bin',T('importBinSet'),importBins,{id:'tbImportBins'}),tbutton('stutter',T('importStutter'),importStutter,{id:'tbImportStutter'}),tsep(),
  el('span',{class:'tlabel',text:T('binSet')}));
 const combo=el('select',{class:'combo',id:'binSetCombo',title:T('binSet')});combo.addEventListener('change',()=>{ui.binSet=combo.value;render()});
 tb.append(combo,tbutton('newBinSet',T('newBinSet'),newBinSet,{id:'tbNewBinSet'}));
 $('applyBtn').textContent=T('apply');$('okBtn').textContent=T('ok');$('cancelBtn').textContent=T('cancel')}

$('applyBtn').addEventListener('click',apply);
$('okBtn').addEventListener('click',async()=>{if(ui.dirty&&!(await apply()))return;window.close()});
$('cancelBtn').addEventListener('click',closeWin);
window.addEventListener('beforeunload',e=>{if(ui.dirty){e.preventDefault();e.returnValue=''}});
window.addEventListener('resize',()=>renderContent());
document.addEventListener('keydown',e=>{if(e.target.closest('input,select,textarea'))return;
 if(e.key==='Delete'&&ui.selBin){deleteBin()}});
window.addEventListener('storage',e=>{if(e.key===KitStore.KEY&&!ui.dirty){saved=KitStore.load();work=KitStore.clone(saved);render()}});
window.PM={state:ui,go,get work(){return work}};
buildChrome();render();
})();
