// Size Match Editor: size-standard peaks, fragment assignments, calibration and SQ override.
(function(){
const SV='http://www.w3.org/2000/svg';
const sv=(tag,attrs={},text)=>{const n=document.createElementNS(SV,tag);for(const [k,v] of Object.entries(attrs))n.setAttribute(k,v);if(text!=null)n.textContent=text;return n};

// sample: {file, analysis, sqOverride}; actions: {override(anchors, reason), clearOverride()}
function open(sample,actions){
 const a=sample.analysis;if(!a||!a.sizing)return message(T('selectAnalyzedSample'));
 const sz=a.sizing;const ref=(a.channels||[]).find(c=>c.tag===sz.reference_channel);
 if(!ref)return message(T('noStandardChannel'));
 const expected=(sz.expected_bp||[]).slice();
 const peaks=(ref.peaks||[]).slice().sort((x,y)=>x.scan-y.scan);
 // current assignments: bp -> scan
 const assign=new Map();
 const src=sz.sq_override?sz.sq_override.anchors.map(([scan,bp])=>({scan,expected_bp:bp})):(sz.matched||[]);
 for(const m of src)if(m.expected_bp!=null&&m.scan!=null)assign.set(Number(m.expected_bp),Number(m.scan));
 let pick=null,selPk=null;
 const orig=new Map(assign);const hist=[];
 const snap=()=>{hist.push(new Map(assign));if(hist.length>50)hist.shift()};
 const undo=()=>{if(!hist.length)return;const h=hist.pop();assign.clear();for(const [k,v] of h)assign.set(k,v);draw()};
 const clearAll=()=>{if(!assign.size)return;snap();assign.clear();pick=null;draw()};
 const autoAdjust=()=>{snap();assign.clear();for(const [k,v] of orig)assign.set(k,v);pick=null;draw()};
 const delPicked=()=>{if(pick==null||!assign.has(pick))return;snap();assign.delete(pick);draw()};
 const scanLabel=sc=>{for(const [k,v] of assign)if(v===sc)return k;return null};
 const sizePicker=(title,cur,cb)=>dialog(title,body=>{const sel=el('select',{id:'smSizeSel'});
   for(const bp of expected)sel.append(new Option(String(bp)+(assign.has(bp)&&bp!==cur?' ('+T('smAssigned')+')':''),String(bp)));sel.value=String(cur??'');body.append(el('div',{class:'field'},el('label',{text:T('smPickSize')}),sel))},
  [[T('ok'),b=>{const n=parseFloat(b.querySelector('#smSizeSel').value);cb(n);return true},true],[T('cancel'),()=>true]]);
 const addLabel=()=>{if(selPk==null||scanLabel(selPk)!=null)return;const first=expected.find(x=>!assign.has(x));
  sizePicker(T('smAddLabel').replace(/\.+$/,''),first,n=>{snap();assign.delete(n);assign.set(n,selPk);selPk=null;draw()})};
 const changeLabel=()=>{const sc=selPk!=null&&scanLabel(selPk)!=null?selPk:(pick!=null&&assign.has(pick)?assign.get(pick):null);if(sc==null)return;const cur=scanLabel(sc);
  sizePicker(T('smChangeLabel').replace(/\.+$/,''),cur,n=>{if(n===cur)return;snap();assign.delete(cur);assign.delete(n);assign.set(n,sc);pick=n;draw()})};
 const yMax=mode=>{const z={...(ZM0().get(ref)||{})};const vals=ref.analyzed_values||ref.values||[];
  if(mode==='max'){delete z.maxY;z.yMode='max'}
  else{let v=null;for(;;){const t=prompt(T('smEnterYMax'),String(Math.round(z.maxY||0)||''));if(t==null)return;const n=parseFloat(String(t).replace(',','.'));if(n>0){v=n;break}}z.maxY=v;z.yMode='custom'}
  ZM0().set(ref,z);draw()};
 const ZM0=()=>window.__smZ=window.__smZ||new WeakMap();
 const checkQuality=()=>{const q=sz.sq_override?1:(sz.base_quality||{}).score;message([`SQ: ${q==null?'NA':Number(q).toFixed(3)}`,`${T('smStatus')}: ${sz.sq_override?T('smOverridden'):sz.status}`,`${T('smMatched')}: ${assign.size}/${expected.length}`,...(sz.reasons||[])].join('\n'),T('smCheckQuality'))};
 const printSize=()=>{const body=d.body;const w=window.open('','opentrace-sm-print','width=1000,height=760');if(!w){message(T('plotWindowBlocked'));return}
  const clone=body.cloneNode(true);clone.querySelectorAll('.smtools,button').forEach(x=>x.remove());
  w.document.open();w.document.write('<!doctype html><html><head><meta charset="utf-8"><title>'+T('sizeMatchEditor').replace(/[<&]/g,'')+'</title><link rel="stylesheet" href="/idx/idx.css"><style>body{background:#fff;overflow:auto;padding:12px}</style></head><body><h3>'+String(sample.file).replace(/[<&]/g,'')+'</h3>'+clone.innerHTML+'</body></html>');
  w.document.close();w.onload=()=>{try{w.focus();w.print()}catch(e){}}};
 const fullView=()=>{const m=window.__smZ;m&&m.delete(ref);draw()};
 const d=dialog(`${T('sizeMatchEditor').replace(/\.+$/,'')} — ${sample.file}`,b=>b.classList.add('smdlg'),[
  [T('smOverride'),async()=>{const anchors=[...assign.entries()].map(([bp,scan])=>[scan,bp]).sort((x,y)=>x[0]-y[0]);
    if(anchors.length<3){message(T('smNeedThree'));return false}
    for(let i=1;i<anchors.length;i++)if(anchors[i][1]<=anchors[i-1][1]){message(T('smOrder'));return false}
    const reason=await askReason();if(reason==null)return false;await actions.override(anchors,reason);return true}],
  ...(sz.sq_override?[[T('smRestore'),async()=>{await actions.clearOverride();return true}]]:[]),
  [T('closeWin'),()=>true,true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();
  const q=sz.sq_override?1:(sz.base_quality||{}).score;
  b.append(el('div',{class:'smhead'},
   el('span',{text:`${T('colSizeStandard')}: ${sample.sizeStd||''}`}),
   el('span',{text:`${T('smStatus')}: ${sz.sq_override?T('smOverridden'):sz.status}`}),
   el('span',{text:`SQ: ${q==null?'NA':Number(q).toFixed(3)}`}),
   el('span',{text:`${T('smMatched')}: ${assign.size}/${expected.length}`})));
  {const tb=el('div',{class:'btnrow smtools'});const bt=(txt,fn,id,dis)=>{const x=el('button',{class:'btn small',type:'button',text:txt,id});x.disabled=!!dis;x.addEventListener('click',fn);return x};
   tb.append(bt(T('smUndo')+' (Ctrl+Z)',undo,'smUndo',!hist.length),bt(T('smDeleteLabel')+' (Del)',delPicked,'smDelLabel',pick==null||!assign.has(pick)),bt(T('smDeleteAll')+' (Ctrl+Del)',clearAll,'smDelAll',!assign.size),bt(T('smAutoAdjust'),autoAdjust,'smAuto'),bt(T('smAddLabel'),addLabel,'smAdd',selPk==null||scanLabel(selPk)!=null),bt(T('smChangeLabel')+' (Ctrl+L)',changeLabel,'smChange',!(selPk!=null&&scanLabel(selPk)!=null)&&!(pick!=null&&assign.has(pick))),bt(T('smCheckQuality'),checkQuality,'smCheck'),bt(T('fullView')+' (Ctrl+])',fullView,'smFull'),bt(T('print'),printSize,'smPrint'),bt(T('scaleMaxY'),()=>yMax('max'),'smYMax'),bt(T('smScaleTo'),()=>yMax('custom'),'smYCustom'));b.append(tb)}
  if((sz.reasons||[]).length)b.append(el('div',{class:'note',text:sz.reasons.join('; ')}));
  if(sz.sq_override)b.append(el('div',{class:'note',text:`${T('smOverrideReason')}: ${sz.sq_override.reason||'—'}`}));
  // electropherogram of the standard dye
  const W=Math.max(700,Math.min(1100,window.innerWidth-120)),H=230;
  const vals=ref.analyzed_values||ref.values||[];
  const ZM=window.__smZ=window.__smZ||new WeakMap();const zs=ZM.get(ref)||{};
  const lo=zs.lo??(peaks.length?Math.max(0,peaks[0].scan-300):0),hi=zs.hi??(peaks.length?Math.min(vals.length-1,peaks[peaks.length-1].scan+300):vals.length-1);
  // Scale to the matched fragments, as the primer peak would flatten the standard.
  const matchedH=peaks.filter(p=>[...assign.values()].includes(p.scan)).map(p=>vals[Math.round(p.scan)]??p.height);
  const maxY=zs.maxY??(zs.yMode==='max'?Math.max(1,...vals.slice(Math.max(0,Math.floor(lo)),Math.ceil(hi)+1).map(v=>v||0))*1.05:null)??Math.max(1,...(matchedH.length?matchedH.map(h=>h*1.25):peaks.map(p=>p.height)));
  const X=s=>(s-lo)/(hi-lo||1)*W,Y=v=>H-20-Math.max(0,Math.min(1.1,v/maxY))*(H-50);
  const svg=sv('svg',{width:W,height:H,class:'smsvg'});
  let path='';const step=Math.max(1,Math.floor((hi-lo)/W));
  for(let s=lo;s<=hi;s+=step)path+=(path?'L':'M')+X(s).toFixed(1)+' '+Y(vals[s]||0).toFixed(1);
  svg.append(sv('path',{d:path,fill:'none',stroke:'#e67e22','stroke-width':1}));
  const byScan=new Map([...assign.entries()].map(([bp,scan])=>[scan,bp]));
  for(const p of peaks){const x=X(p.scan),y=Y(vals[Math.round(p.scan)]??p.height);const bp=byScan.get(p.scan);
   const g=sv('g',{class:'smpk'+(bp!=null?' on':'')});
   g.append(sv('line',{x1:x,x2:x,y1:y,y2:H-20,stroke:'transparent','stroke-width':8}),sv('circle',{cx:x,cy:y,r:bp!=null?4:3,fill:bp!=null?'#e67e22':'#bbb',stroke:selPk===p.scan?'#d00':'#555','stroke-width':selPk===p.scan?3:1}));
   if(bp!=null)g.append(sv('text',{x,y:y-8,'text-anchor':'middle','font-size':10},String(bp)));
   g.append(sv('title',{},`scan ${p.scan}, ${Math.round(p.height)} RFU`));
   g.addEventListener('click',()=>{if(pick==null){selPk=selPk===p.scan?null:p.scan;draw();return}selPk=null;snap();for(const [k,v] of assign)if(v===p.scan)assign.delete(k);assign.set(pick,p.scan);
    const i=expected.indexOf(pick);pick=expected.slice(i+1).find(x=>!assign.has(x))??null;draw()});
   svg.append(g)}
  svg.append(sv('line',{x1:0,x2:W,y1:H-20,y2:H-20,stroke:'#555'}));
  // Axis zones as in ID-X SizeMatchedPlot: drag along the bottom strip zooms scans, drag in the left strip zooms the height; double click on a strip restores it.
  {const zone=(x,y)=>y>=H-20?'x':x<36?'y':'plot';let d=null;const pt=e=>{const r=svg.getBoundingClientRect();return [e.clientX-r.left,e.clientY-r.top]};
   svg.addEventListener('mousedown',e=>{const [x,y]=pt(e);d={z:zone(x,y),x,y}});
   svg.addEventListener('mouseup',e=>{if(!d)return;const q=d;d=null;const [x,y]=pt(e);const z={...(ZM.get(ref)||{})};
    if(q.z==='x'&&Math.abs(x-q.x)>4){z.lo=lo+Math.min(q.x,x)/W*(hi-lo);z.hi=lo+Math.max(q.x,x)/W*(hi-lo);ZM.set(ref,z);draw()}
    else if(q.z==='y'&&Math.abs(y-q.y)>4){const top=Math.min(q.y,y);z.maxY=Math.max(1,maxY*(H-20-top)/(H-50));ZM.set(ref,z);draw()}});
   svg.addEventListener('dblclick',e=>{const [x,y]=pt(e);const z=zone(x,y);if(z==='plot')return;const o={...(ZM.get(ref)||{})};if(z==='x'){delete o.lo;delete o.hi}else delete o.maxY;ZM.set(ref,o);draw()})}
  b.append(el('div',{class:'pmplot'},svg),el('div',{class:'note',text:T('smHint')}));
  // fragment table + calibration curve
  const t=el('table',{class:'g smtab'});t.append(el('thead',{},el('tr',{},...[T('colSize'),T('smScan'),T('colHeight'),''].map(h=>el('th',{text:h})))));
  const tb=el('tbody');
  for(const bp of expected){const scan=assign.get(bp);const p=peaks.find(x=>x.scan===scan);
   const tr=el('tr',{class:pick===bp?'sel':''},el('td',{class:'num',text:String(bp)}),el('td',{class:'num',text:scan!=null?String(scan):'—'}),
    el('td',{class:'num',text:p?String(Math.round(p.height)):''}));
   const un=el('button',{class:'btn small',type:'button',text:T('smUnassign')});un.disabled=scan==null;
   un.addEventListener('mousedown',e=>e.stopPropagation());un.addEventListener('click',e=>{e.stopPropagation();snap();assign.delete(bp);draw()});tr.append(el('td',{},un));
   tr.addEventListener('mousedown',()=>{pick=bp;draw()});tb.append(tr)}
  t.append(tb);
  const pts=[...assign.entries()].map(([bp,scan])=>[scan,bp]).sort((x,y)=>x[0]-y[0]);
  const C=sv('svg',{width:300,height:240,class:'smsvg'});
  if(pts.length>1){const xs=pts.map(p=>p[0]),ys=pts.map(p=>p[1]);const x0=Math.min(...xs),x1=Math.max(...xs),y0=Math.min(...ys),y1=Math.max(...ys);
   const cx=s=>20+(s-x0)/(x1-x0||1)*270,cy=v=>220-(v-y0)/(y1-y0||1)*200;
   C.append(sv('path',{d:pts.map((p,i)=>(i?'L':'M')+cx(p[0]).toFixed(1)+' '+cy(p[1]).toFixed(1)).join(''),stroke:'#1f4fd6',fill:'none'}));
   for(const p of pts)C.append(sv('circle',{cx:cx(p[0]),cy:cy(p[1]),r:3,fill:'#e67e22'}));
   C.append(sv('text',{x:20,y:236,'font-size':10},`${x0}–${x1} scan`),sv('text',{x:4,y:12,'font-size':10},`${y0}–${y1} bp`))}
  b.append(el('div',{class:'smcols'},el('div',{class:'grid smgrid'},t),el('div',{},el('div',{class:'cap',text:T('smCurve')}),C)))};
 // ID-X Size Match Editor keys: Delete = Delete Size Label, Ctrl+Delete = Delete All Size Labels, Ctrl+Z = Undo, Ctrl+Shift+O = Override All SQ, Ctrl+] = Full View
 const keys=e=>{if(!document.body.contains(d.body)){document.removeEventListener('keydown',keys);return}if(e.target.closest('input,textarea,select'))return;
  const ctrl=e.ctrlKey||e.metaKey;
  if(e.key==='Delete'){e.preventDefault();ctrl?clearAll():delPicked();return}
  if(!ctrl)return;const code=e.code;
  if(code==='KeyZ'&&!e.shiftKey){e.preventDefault();undo()}
  else if(code==='KeyO'&&e.shiftKey){e.preventDefault();d.body.closest('.dlg')?.querySelector('.foot .btn')?.click()}
  else if(e.key===']'){e.preventDefault();fullView()}
  else if(code==='KeyL'){e.preventDefault();changeLabel()}};
 document.addEventListener('keydown',keys);
 draw();return d}
function askReason(){return new Promise(res=>{dialog(T('smOverride'),b=>b.append(el('div',{class:'field'},el('label',{text:T('reasonForChange')}),el('input',{type:'text',id:'smReason'}))),
 [[T('ok'),b=>{const v=b.querySelector('#smReason').value.trim();if(!v){message(T('reasonRequired'));return false}res(v);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
window.SizeMatch={open};
})();
