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
function newKit(){askName(T('newKitTitle'),T('kitName'),'',v=>{if(kitOf(v)){message(fmt('kitExists',v));return false}
 work.kits.push({name:v,type:'MICROSATELLITE',comment:'',panelsVersion:'GMID-X v 1.4',panels:[],binSets:[],stutter:null});
 ui.sel={type:'kit',kit:v};touch();render()},
 b=>b.append(el('div',{class:'field'},el('label',{text:T('kitType')}),el('select',{disabled:''},new Option('Microsatellite','MICROSATELLITE')))))}
function newPanel(){const k=selKit();if(!k)return message(T('selectKitFirst'));
 askName(T('newPanel'),T('panelName'),'',v=>{if(k.panels.some(p=>p.name===v)){message(fmt('nameUsed',v));return false}
  k.panels.push({name:v,comment:'',markers:[]});ui.sel={type:'panel',kit:k.name,panel:v};ui.expanded.add(k.name);touch();render()})}
function newBinSet(){const k=selKit();if(!k)return message(T('selectKitFirst'));
 askName(T('newBinSetTitle'),T('binSetName'),'',v=>{if(k.binSets.some(s=>s.name===v)){message(fmt('binSetExists',v));return false}
  k.binSets.push({name:v,version:'GMID-X v 1.2',panels:{}});ui.binSet=v;touch();render()})}
function newMarker(){const k=selKit(),p=selPanel();if(!p)return message(T('selectPanelFirst'));if(!curSet())return message(T('createBinSetFirst'));
 askName(T('newMarker'),T('markerName'),'',v=>{if(p.markers.some(m=>m.name===v)){message(T('markerExists'));return false}
  const last=p.markers[p.markers.length-1];const lo=last?last.max_bp+5:100;
  p.markers.push({name:v,dye:'blue',min_bp:lo,max_bp:lo+40,control_alleles:[],repeat:4,option:'none',y_marker:false,ladder_alleles:[]});
  ui.sel={type:'marker',kit:k.name,panel:p.name,marker:v};touch();render()})}
function deleteNode(){
 const s=ui.sel,k=selKit();
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
function binDialog(existing){
 const k=selKit(),p=selPanel(),m=selMarker(),set=curSet();
 if(!m)return message(T('selectMarkerFirst'));if(!set)return message(T('selectBinSetFirst'));
 const bins=set.panels[p.name]?.[m.name]||[];
 const b0=existing||{label:'',center_bp:+(((m.min_bp+m.max_bp)/2).toFixed(2)),left_bp:0.5,right_bp:0.5,virtual:false};
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
 const kits=work.kits.map(k=>node('kit',k.name,s.type==='kit'&&s.kit===k.name,()=>go({type:'kit',kit:k.name}),
  (s.kit===k.name||ui.expanded.has(k.name))?k.panels.map(p=>node('panel',p.name,s.type==='panel'&&s.kit===k.name&&s.panel===p.name,()=>go({type:'panel',kit:k.name,panel:p.name}),
   (s.kit===k.name&&s.panel===p.name)?p.markers.map(m=>node('marker',m.name,s.type==='marker'&&s.marker===m.name,()=>go({type:'marker',kit:k.name,panel:p.name,marker:m.name}))):[])):[]));
 root.append(node('panelManager',T('panelMgrTitle'),s.type==='root',()=>go({type:'root'}),kits));nav.append(root)}
function go(sel){if(sel.kit&&sel.kit!==ui.sel.kit)ui.binSet=null;ui.sel=sel;ui.selBin=null;ui.selRow=null;if(sel.kit)ui.expanded.add(sel.kit);ui.view=null;render()}

// ---------- tables ----------
function table(cols,rows,onSel,selIndex){
 const t=el('table',{class:'g'});const cg=el('colgroup');for(const [,w] of cols)cg.append(el('col',{style:`width:${w}px`}));t.append(cg);
 const hr=el('tr');hr.append(el('th',{text:'',style:'width:28px'}));for(const [h] of cols)hr.append(el('th',{text:h,title:h}));
 t.querySelector('colgroup').prepend(el('col',{style:'width:28px'}));t.append(el('thead',{},hr));
 const tb=el('tbody');rows.forEach((r,i)=>{const tr=el('tr',{class:i===selIndex?'sel':''});tr.append(el('td',{class:'rh',text:String(i+1)}));
  for(const c of r){const td=el('td');if(c instanceof Node)td.append(c);else{td.textContent=c==null?'':String(c);td.title=td.textContent}tr.append(td)}
  if(onSel)tr.addEventListener('mousedown',()=>onSel(i));tb.append(tr)});
 t.append(tb);return t}
function dyeCell(d){const s=el('span',{class:'dye'});s.append(el('i',{style:`background:${DYE_CSS[d]||'#888'}`}),document.createTextNode(T('dye_'+d)));return s}
function stutterSummary(k,p,m){const st=stutterOf(k,p,m).filter(s=>s.direction==='minus');return st.length?st.map(s=>s.ratio).join(', '):''}
function markerTable(k,p,only){
 const cols=[[T('markerName'),120],[T('dyeColor'),80],[T('minSize'),70],[T('maxSize'),70],[T('controlAlleles'),110],[T('markerRepeat'),60],
  [T('stutterPct'),110],[T('ladderAlleles'),330],[T('yMarker'),60],[T('internalQc'),70],[T('comments'),110]];
 const list=only?[only]:p.markers;
 const rows=list.map(m=>[m.name,dyeCell(m.dye),m.min_bp,m.max_bp,m.control_alleles.join(','),m.repeat,stutterSummary(k,p.name,m.name),
  m.ladder_alleles.join(', '),m.y_marker?'true':'false','internal_qc' in m?String(m.internal_qc):'',m.comment||'']);
 const t=table(cols,rows,i=>{ui.selRow=i;render()},ui.selRow);
 t.querySelectorAll('tbody tr').forEach((tr,i)=>tr.addEventListener('dblclick',()=>editMarker(k,p,list[i])));
 return t}
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
  svg.append(g)}
 if(!bins||!bins.length)svg.append(sv('text',{x:W/2,y:H/2,'text-anchor':'middle',fill:'#777','font-size':12},T('markerNoBins')));
 // zoom: drag on the ruler; wheel zooms around the pointer
 let start=null;const band=sv('rect',{y:0,height:H,fill:'rgba(10,36,106,.15)',visibility:'hidden'});svg.append(band);
 const bp=e=>lo+(e.offsetX/W)*(hi-lo);
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
  c.append(table([[T('kitName'),260],[T('kitType'),140],[T('comments'),260]],work.kits.map(x=>[x.name,'MICROSATELLITE',x.comment||'']),i=>go({type:'kit',kit:work.kits[i].name})));
  if(!work.kits.length)c.append(el('div',{class:'empty',text:T('panelHint')}));
 }else if(s.type==='kit'&&k){
  c.append(table([[T('panelName'),260],[T('markersCol'),80],[T('binSets'),260],[T('comments'),200]],
   k.panels.map(x=>[x.name,x.markers.length,k.binSets.filter(b=>b.panels[x.name]).map(b=>b.name).join(', '),x.comment||'']),i=>go({type:'panel',kit:k.name,panel:k.panels[i].name})));
  if(!k.binSets.length)c.append(el('div',{class:'note',text:T('importBinsHint')}));
 }else if(s.type==='panel'&&p){
  if(p.markers.length)c.append(panelPlot(k,p));
  c.append(el('div',{class:'grid pmgrid'},markerTable(k,p)));
 }else if(s.type==='marker'&&m){
  c.append(binPlot(k,p,m),el('div',{class:'grid pmgrid small'},markerTable(k,p,m)),stutterTables(k,p,m));
 }}
function renderBinSetCombo(){
 const combo=$('binSetCombo');combo.replaceChildren();const k=selKit();
 for(const s of k?.binSets||[])combo.append(new Option(s.name,s.name));
 combo.disabled=!k||!k.binSets.length;const set=curSet();if(set){combo.value=set.name;ui.binSet=set.name}}
function render(){renderNavigator();renderBinSetCombo();renderContent();
 const s=ui.sel;$('markerInfo').textContent=s.type==='marker'?`${s.kit} › ${s.panel} › ${s.marker}`:s.type==='panel'?`${s.kit} › ${s.panel}`:s.kit||'';
 $('applyBtn').disabled=!ui.dirty}

// ---------- commit ----------
async function apply(){
 if(window.Sec&&!Sec.can('UPDATE_PANEL')){Sec.denied();return false}
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
  {label:T('file'),items:[{label:T('newKit'),run:newKit},'-',{label:T('importPanels'),run:importPanels},{label:T('importBinSet'),run:importBins},{label:T('importStutter'),run:importStutter},'-',
   {label:T('exportPanels'),run:exportPanels},{label:T('exportBinSet'),run:exportBinSet},{label:T('exportStutter'),run:exportStutter},{label:T('exportAllKits'),run:exportAll},'-',
   {label:T('print'),run:()=>window.print()},'-',{label:T('closeWin'),run:closeWin}]},
  {label:T('edit'),items:[{label:T('undo'),disabled:true},'-',{label:T('newPanel'),run:newPanel},{label:T('newMarker'),run:newMarker},'-',{label:T('deleteItem'),run:deleteNode}]},
  {label:T('binsMenu'),items:[{label:T('newBinSet'),run:newBinSet},{label:T('deleteBinSet'),run:deleteBinSet},'-',{label:T('addBin'),run:()=>binDialog(null)},
   {label:T('editBin'),run:editBin},{label:T('deleteBin'),run:deleteBin},'-',{label:T('autoBin'),disabled:true},{label:T('addReferenceData'),disabled:true}]},
  {label:T('view'),items:[{label:T('showNavigator'),checked:!ui.hideNav,run:()=>{ui.hideNav=!ui.hideNav;buildChrome();render()}},{label:T('fullView'),run:()=>{ui.view=null;render()}},'-',
   {label:T('language'),sub:[{label:'Русский',checked:I18N.lang==='ru',run:()=>{I18N.set('ru');buildChrome();render()}},{label:'English',checked:I18N.lang==='en',run:()=>{I18N.set('en');buildChrome();render()}}]}]},
  {label:T('help'),items:[{label:T('about'),run:()=>message(T('panelHint'),T('panelMgrTitle'))}]}]);
 const tb=$('toolbar');tb.replaceChildren(
  tbutton('panel',T('importPanels'),importPanels,{id:'tbImportPanels'}),tbutton('bin',T('importBinSet'),importBins,{id:'tbImportBins'}),tbutton('stutter',T('importStutter'),importStutter,{id:'tbImportStutter'}),tsep(),
  el('span',{class:'tlabel',text:T('binSet')}));
 const combo=el('select',{class:'combo',id:'binSetCombo',title:T('binSet')});combo.addEventListener('change',()=>{ui.binSet=combo.value;render()});
 tb.append(combo,tsep(),tbutton('fullView',T('fullView'),()=>{ui.view=null;render()}));
 $('applyBtn').textContent=T('apply');$('okBtn').textContent=T('ok');$('cancelBtn').textContent=T('cancel')}

$('applyBtn').addEventListener('click',apply);
$('okBtn').addEventListener('click',async()=>{if(ui.dirty&&!(await apply()))return;window.close()});
$('cancelBtn').addEventListener('click',closeWin);
window.addEventListener('beforeunload',e=>{if(ui.dirty){e.preventDefault();e.returnValue=''}});
window.addEventListener('resize',()=>renderContent());
document.addEventListener('keydown',e=>{if(e.target.closest('input,select,textarea'))return;
 if(e.key==='Delete'&&ui.selBin){deleteBin()}});
window.addEventListener('storage',e=>{if(e.key===KitStore.KEY&&!ui.dirty){saved=KitStore.load();work=KitStore.clone(saved);render()}});
window.PM={state:ui,get work(){return work}};
buildChrome();render();
})();
