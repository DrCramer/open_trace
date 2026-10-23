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
 let pick=null;
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
  if((sz.reasons||[]).length)b.append(el('div',{class:'note',text:sz.reasons.join('; ')}));
  if(sz.sq_override)b.append(el('div',{class:'note',text:`${T('smOverrideReason')}: ${sz.sq_override.reason||'—'}`}));
  // electropherogram of the standard dye
  const W=Math.max(700,Math.min(1100,window.innerWidth-120)),H=230;
  const vals=ref.analyzed_values||ref.values||[];
  const lo=peaks.length?Math.max(0,peaks[0].scan-300):0,hi=peaks.length?Math.min(vals.length-1,peaks[peaks.length-1].scan+300):vals.length-1;
  // Scale to the matched fragments, as the primer peak would flatten the standard.
  const matchedH=peaks.filter(p=>[...assign.values()].includes(p.scan)).map(p=>vals[Math.round(p.scan)]??p.height);
  const maxY=Math.max(1,...(matchedH.length?matchedH.map(h=>h*1.25):peaks.map(p=>p.height)));
  const X=s=>(s-lo)/(hi-lo||1)*W,Y=v=>H-20-Math.max(0,Math.min(1.1,v/maxY))*(H-50);
  const svg=sv('svg',{width:W,height:H,class:'smsvg'});
  let path='';const step=Math.max(1,Math.floor((hi-lo)/W));
  for(let s=lo;s<=hi;s+=step)path+=(path?'L':'M')+X(s).toFixed(1)+' '+Y(vals[s]||0).toFixed(1);
  svg.append(sv('path',{d:path,fill:'none',stroke:'#e67e22','stroke-width':1}));
  const byScan=new Map([...assign.entries()].map(([bp,scan])=>[scan,bp]));
  for(const p of peaks){const x=X(p.scan),y=Y(vals[Math.round(p.scan)]??p.height);const bp=byScan.get(p.scan);
   const g=sv('g',{class:'smpk'+(bp!=null?' on':'')});
   g.append(sv('line',{x1:x,x2:x,y1:y,y2:H-20,stroke:'transparent','stroke-width':8}),sv('circle',{cx:x,cy:y,r:bp!=null?4:3,fill:bp!=null?'#e67e22':'#bbb',stroke:'#555'}));
   if(bp!=null)g.append(sv('text',{x,y:y-8,'text-anchor':'middle','font-size':10},String(bp)));
   g.append(sv('title',{},`scan ${p.scan}, ${Math.round(p.height)} RFU`));
   g.addEventListener('click',()=>{if(pick==null)return;for(const [k,v] of assign)if(v===p.scan)assign.delete(k);assign.set(pick,p.scan);
    const i=expected.indexOf(pick);pick=expected.slice(i+1).find(x=>!assign.has(x))??null;draw()});
   svg.append(g)}
  svg.append(sv('line',{x1:0,x2:W,y1:H-20,y2:H-20,stroke:'#555'}));
  b.append(el('div',{class:'pmplot'},svg),el('div',{class:'note',text:T('smHint')}));
  // fragment table + calibration curve
  const t=el('table',{class:'g smtab'});t.append(el('thead',{},el('tr',{},...[T('colSize'),T('smScan'),T('colHeight'),''].map(h=>el('th',{text:h})))));
  const tb=el('tbody');
  for(const bp of expected){const scan=assign.get(bp);const p=peaks.find(x=>x.scan===scan);
   const tr=el('tr',{class:pick===bp?'sel':''},el('td',{class:'num',text:String(bp)}),el('td',{class:'num',text:scan!=null?String(scan):'—'}),
    el('td',{class:'num',text:p?String(Math.round(p.height)):''}));
   const un=el('button',{class:'btn small',type:'button',text:T('smUnassign')});un.disabled=scan==null;
   un.addEventListener('mousedown',e=>e.stopPropagation());un.addEventListener('click',e=>{e.stopPropagation();assign.delete(bp);draw()});tr.append(el('td',{},un));
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
 draw();return d}
function askReason(){return new Promise(res=>{dialog(T('smOverride'),b=>b.append(el('div',{class:'field'},el('label',{text:T('reasonForChange')}),el('input',{type:'text',id:'smReason'}))),
 [[T('ok'),b=>{const v=b.querySelector('#smReason').value.trim();if(!v){message(T('reasonRequired'));return false}res(v);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
window.SizeMatch={open};
})();
