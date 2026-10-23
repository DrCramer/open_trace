// OpenTrace Samples Plot window.
(function(){
const $=id=>document.getElementById(id);
const DYE_COLORS={DATA1:'#0000ff',DATA2:'#00a000',DATA3:'#000000',DATA4:'#ff0000',DATA105:'#b000d0',DATA106:'#ff8000'};
const DYE_KEYS=['DATA1','DATA2','DATA3','DATA4','DATA105','DATA106'];
const DYE_NAMES={DATA1:'blue',DATA2:'green',DATA3:'yellow',DATA4:'red',DATA105:'purple',DATA106:'orange'};
const GQ_FILL={pass:'#3ccf4e',check:'#f5c542',low:'#ff4d4d',na:'#c8c8c8'};
const PQV_KEYS=['SOS','SQ','SSPK','MIX','OMR','CGQ'];
const METRIC_PQV={allele_number:'AN',absolute_height:'LPH',high_height:'MPH',spike:'SPK',ratio:'PHR',broad:'BD',offscale:'OS',overlap:'OVL',binning:'BIN'};

let payload=null;
const view={panes:2,layout:'separate',labels:'h',bins:true,header:true,markerRange:true,table:'genotypes',dyes:new Set(DYE_KEYS),
 x0:null,x1:null,selMarker:null,selSample:null};

function load(){
 try{payload=window.opener&&window.opener.OT?window.opener.OT.plotPayload():null}catch(e){payload=null}
 if(payload?.lang)I18N.set(payload.lang);
 build();draw();printHead()}

// ---------- scan <-> bp ----------
function curveOf(a){const anchors=a?.sizing?.anchors;if(!anchors||anchors.length<2)return null;return [...anchors].sort((p,q)=>p[0]-q[0])}
function scanToBp(c,s){if(!c)return s;let i=1;while(i<c.length-1&&s>c[i][0])i++;const [s0,b0]=c[i-1],[s1,b1]=c[i];return b0+(s-s0)*(b1-b0)/(s1-s0)}
function bpRange(){const ms=payload?.kit?.markers||[];
 if(ms.length)return [Math.min(...ms.map(m=>m.min_bp))-10,Math.max(...ms.map(m=>m.max_bp))+10];
 for(const s of payload?.samples||[]){const c=curveOf(s.analysis);if(c)return [c[0][1],c[c.length-1][1]]}
 return [0,500]}

// ---------- panes ----------
function paneList(){
 const out=[];
 for(const s of payload?.samples||[]){
  const tags=(s.analysis?.channels||[]).filter(c=>c.group!=='auxiliary'&&DYE_KEYS.includes(c.tag)).map(c=>c.tag);
  const shown=(tags.length?tags:DYE_KEYS.slice(0,4)).filter(t=>view.dyes.has(t)&&t!==s.analysis?.sizing?.reference_channel);
  if(view.layout==='separate')for(const t of shown)out.push({sample:s,dyes:[t]});
  else out.push({sample:s,dyes:shown})}
 if(view.layout==='overlay')return out.length?[{overlay:true,items:out}]:[];
 return out}

function draw(){
 const area=$('plotarea');area.replaceChildren();
 if(!payload||!payload.samples.length){area.append(el('div',{class:'empty',text:T('selectSamplesFirst')}));renderLower();return}
 const panes=paneList();
 const h=Math.max(150,Math.floor((area.clientHeight-2)/Math.max(1,view.panes)));
 const [d0,d1]=bpRange();const x0=view.x0??d0,x1=view.x1??d1;
 for(const p of panes){
  const items=p.overlay?p.items:[p];const s=items[0].sample;
  const pane=el('div',{class:'pane'});
  if(view.header){
   const head=el('div',{class:'phead'});
   head.append(el('div',{class:'c',style:'width:250px',text:p.overlay?'':s.file}),el('div',{class:'c',style:'width:150px',text:p.overlay?'':s.name}));
   for(const k of PQV_KEYS){const c=el('div',{class:'c',style:'width:46px;justify-content:center'});c.innerHTML=p.overlay?'':pqv(s.pqv?.[k]);head.append(c)}
   if(!p.overlay){const lab=el('label',{class:'c',style:'gap:4px'});lab.append(el('input',{type:'checkbox'}),T('markSample'));head.append(lab);head._sample=s;head.title=T('headerMenuHint')}
   pane.append(head)}
  const canvas=el('canvas');canvas.height=h;pane.append(canvas);area.append(pane);
  requestAnimationFrame(()=>paint(canvas,items,x0,x1))}
 renderLower()}

function paint(cv,items,x0,x1){
 const dpr=window.devicePixelRatio||1,W=cv.clientWidth,H=cv.height;cv.width=W*dpr;cv.height=H*dpr;cv.style.height=H+'px';
 const g=cv.getContext('2d');g.scale(dpr,dpr);g.fillStyle='#fff';g.fillRect(0,0,W,H);g.font='11px Tahoma,Arial';
 const L=50,R=12,top=view.markerRange?36:16,bottom=H-(view.labels==='none'?16:46);
 const X=bp=>L+(bp-x0)/(x1-x0)*(W-L-R);
 const s=items[0].sample;const markers=((s.kit||payload.kit)?.markers||[]);
 const dyes=items.flatMap(i=>i.dyes);
 // marker headers + bins
 for(const m of markers.filter(m=>dyes.includes(m.channel))){
  const row=(s.markers||[]).find(r=>r.marker===m.name);const xa=X(m.min_bp),xb=X(m.max_bp);
  if(view.markerRange){g.fillStyle=GQ_FILL[row?.gqCat||'na'];g.fillRect(xa,4,xb-xa,14);g.strokeStyle='#555';g.strokeRect(xa+.5,4.5,xb-xa-1,13);
   g.fillStyle='#000';g.save();g.beginPath();g.rect(xa,4,xb-xa,14);g.clip();g.fillText(m.name,xa+3,15);g.restore();
   m._hit={x0:xa,x1:xb,y0:4,y1:18}}
  if(view.bins)for(const b of m.bins||[]){const ba=X(b.center_bp-(b.left_bp??.5)),bb=X(b.center_bp+(b.right_bp??.5));
   g.fillStyle=b.virtual?'rgba(255,150,190,.35)':'rgba(150,150,150,.28)';g.fillRect(ba,top,Math.max(1,bb-ba),bottom-top)}}
 // axes
 g.strokeStyle='#000';g.beginPath();g.moveTo(L,top);g.lineTo(L,bottom);g.lineTo(W-R,bottom);g.stroke();
 const step=niceStep((x1-x0)/8);g.fillStyle='#000';g.textAlign='center';
 for(let v=Math.ceil(x0/step)*step;v<=x1;v+=step){const x=X(v);g.fillText(String(+v.toFixed(1)),x,top-3);g.beginPath();g.moveTo(x,top);g.lineTo(x,top+3);g.stroke()}
 // traces
 let ymax=1;const traces=[];
 for(const it of items){const a=it.sample.analysis,c=curveOf(a);
  for(const tag of it.dyes){const ch=(a?.channels||[]).find(q=>q.tag===tag);const values=ch?.analyzed_values||ch?.values;if(!values)continue;
   const pts=[];values.forEach((v,i)=>{const bp=scanToBp(c,i);if(bp>=x0&&bp<=x1)pts.push([bp,v])});
   for(const [,v] of pts)ymax=Math.max(ymax,v);traces.push({tag,pts,peaks:ch.peaks||[],sample:it.sample})}}
 if(!traces.length){g.fillStyle='#777';g.textAlign='center';g.fillText(T('noValues'),W/2,(top+bottom)/2);return}
 ymax*=1.08;const Y=v=>bottom-(v/ymax)*(bottom-top);
 g.textAlign='right';const ys=niceStep(ymax/4);for(let v=0;v<=ymax;v+=ys){const y=Y(v);g.fillText(String(Math.round(v)),L-4,y+4);g.beginPath();g.moveTo(L-3,y);g.lineTo(L,y);g.stroke()}
 g.save();g.beginPath();g.rect(L,top,W-L-R,bottom-top);g.clip();
 for(const t of traces){g.strokeStyle=DYE_COLORS[t.tag]||'#333';g.lineWidth=1;g.beginPath();t.pts.forEach(([bp,v],i)=>i?g.lineTo(X(bp),Y(v)):g.moveTo(X(bp),Y(v)));g.stroke()}
 g.restore();
 // allele labels in boxes under the axis
 const labelBoxes=[];
 if(view.labels!=='none'){g.textAlign='center';const placed=[];
  for(const t of traces){const calls=(t.sample.report?.calls||[]).filter(c=>c.channel===t.tag&&c.size_bp>=x0&&c.size_bp<=x1);
   for(const c of calls){const x=X(c.size_bp);let row=0;while(placed.some(p=>p.row===row&&Math.abs(p.x-x)<26))row++;placed.push({x,row});
    const y=bottom+6+row*16;const w=Math.max(18,g.measureText(c.label).width+8);
    g.strokeStyle=(c.flags||[]).length?'#e040a0':'#000';g.fillStyle='#fff';g.fillRect(x-w/2,y,w,14);g.strokeRect(x-w/2+.5,y+.5,w-1,13);
    g.fillStyle=c.user_edited?'#b00060':'#000';g.fillText(c.label,x,y+11);g.beginPath();g.moveTo(x,bottom);g.lineTo(x,y);g.strokeStyle='#999';g.stroke();
    labelBoxes.push({x0:x-w/2,x1:x+w/2,y0:y,y1:y+14,call:c,sample:t.sample})}}}
 const peakPts=[];for(const t of traces)for(const pk of t.peaks)if(pk.size_bp!=null&&pk.size_bp>=x0&&pk.size_bp<=x1)peakPts.push({x:X(pk.size_bp),y:Y(pk.height),peak:pk,tag:t.tag,sample:t.sample});
 cv._geom={X,x0,x1,L,R,W,top,bottom,ymax,markers:markers.filter(m=>dyes.includes(m.channel)),sample:s,labelBoxes,peakPts};
}
function niceStep(raw){const p=Math.pow(10,Math.floor(Math.log10(raw||1)));const n=raw/p;return (n<1.5?1:n<3.5?2:n<7.5?5:10)*p}

// ---------- interaction ----------
let drag=null;
document.addEventListener('mousedown',e=>{const cv=e.target.closest('canvas');if(!cv?._geom)return;const r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top,G=cv._geom;
 const m=G.markers.find(m=>m._hit&&x>=m._hit.x0&&x<=m._hit.x1&&y>=m._hit.y0&&y<=m._hit.y1);
 if(m){view.selMarker=m.name;view.selSample=G.sample.id;renderLower();return}
 if(y<G.top+4){drag={cv,x0:x};e.preventDefault()}});
document.addEventListener('mouseup',e=>{if(!drag)return;const G=drag.cv._geom,r=drag.cv.getBoundingClientRect(),x=e.clientX-r.left;
 const inv=px=>G.x0+(px-G.L)/(G.W-G.L-G.R)*(G.x1-G.x0);const a=inv(Math.min(drag.x0,x)),b=inv(Math.max(drag.x0,x));drag=null;
 if(b-a>1){view.x0=a;view.x1=b;draw()}});
document.addEventListener('dblclick',e=>{if(e.target.closest('canvas')){view.x0=view.x1=null;draw()}});
document.addEventListener('mousemove',e=>{const cv=e.target.closest?.('canvas');if(!cv?._geom)return;const G=cv._geom,r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
 const bp=G.x0+(x-G.L)/(G.W-G.L-G.R)*(G.x1-G.x0),v=(G.bottom-y)/(G.bottom-G.top)*G.ymax;
 $('xy').textContent=`[X ${bp.toFixed(2)}  Y ${Math.round(v)}]`;
 const m=G.markers.find(m=>bp>=m.min_bp&&bp<=m.max_bp);$('markerInfo').textContent=m?`[${T('colMarker')}: ${m.name} (${m.min_bp.toFixed(2)}-${m.max_bp.toFixed(2)})]`:''});
// ---------- allele label editing (context menu) ----------
function ctxMenu(x,y,items){document.querySelectorAll('.menu.ctx').forEach(m=>m.remove());
 const m=el('div',{class:'menu ctx show',style:`left:${x}px;top:${y}px`});
 for(const it of items){const r=el('div',{class:'mi'+(it.disabled?' dis':''),text:it.label});if(!it.disabled)r.addEventListener('click',()=>{m.remove();it.run()});m.append(r)}
 document.body.append(m);setTimeout(()=>document.addEventListener('mousedown',function h(e){if(!m.contains(e.target)){m.remove();document.removeEventListener('mousedown',h)}}),0)}
function ask(title,fields){return new Promise(res=>{dialog(title,b=>{for(const f of fields)b.append(el('div',{class:'field'},el('label',{text:f.label}),el('input',{type:'text',id:f.id,value:f.value||''})))},
 [[T('ok'),b=>{const out={};for(const f of fields){out[f.id]=b.querySelector('#'+f.id).value.trim();if(!out[f.id]){message(T('reasonRequired'));return false}}
   res(out);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
async function edit(sample,spec){
 let user='';try{user=localStorage.getItem('opentrace.user')||''}catch(e){}
 const fields=[];if(spec.action==='rename'||spec.action==='add')fields.push({id:'edLabel',label:T('colAllele'),value:spec.new});
 fields.push({id:'edUser',label:T('userName'),value:user},{id:'edReason',label:T('reasonForChange')});
 const v=await ask(T('act_'+spec.action),fields);if(!v)return;
 if(v.edLabel&&v.edLabel.length>40)return message(T('badBinName'));
 try{localStorage.setItem('opentrace.user',v.edUser)}catch(e){}
 spec={...spec,new:v.edLabel??spec.new,user:v.edUser,reason:v.edReason};
 let ok=false;try{ok=await window.opener.OT.edit(sample.id,spec)}catch(e){message(String(e.message||e))}
 if(ok){try{payload=window.opener.OT.plotPayload()}catch(e){}build();draw()}}
function binLabel(marker,bp){const b=(marker.bins||[]).find(b=>bp>=b.center_bp-(b.left_bp??.5)&&bp<=b.center_bp+(b.right_bp??.5));return b?b.label:'OL'}
document.addEventListener('contextmenu',e=>{const cv=e.target.closest('canvas');const head=e.target.closest('.phead');
 if(head&&head._sample){e.preventDefault();const s=head._sample;if(!s.editable)return;
  ctxMenu(e.clientX,e.clientY,[{label:T('overrideCgq'),run:()=>edit(s,{action:'override_cgq'})}]);return}
 if(!cv?._geom)return;e.preventDefault();const G=cv._geom,r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
 const s=G.sample;if(!s.editable)return;
 const box=G.labelBoxes.find(b=>x>=b.x0&&x<=b.x1&&y>=b.y0&&y<=b.y1);
 if(box){const c=box.call;ctxMenu(e.clientX,e.clientY,[{label:T('renameAllele')+' ('+c.label+')',run:()=>edit(box.sample,{action:'rename',marker:c.marker,callId:c.id,new:c.label})},
   {label:T('deleteAllele')+' ('+c.label+')',run:()=>edit(box.sample,{action:'delete',marker:c.marker,callId:c.id})}]);return}
 const mk=G.markers.find(m=>m._hit&&x>=m._hit.x0&&x<=m._hit.x1&&y>=m._hit.y0&&y<=m._hit.y1);
 if(mk){ctxMenu(e.clientX,e.clientY,[{label:T('overrideGq')+' ('+mk.name+')',run:()=>edit(s,{action:'override_gq',marker:mk.name})}]);return}
 const near=G.peakPts.filter(p=>Math.abs(p.x-x)<=6).sort((a,b)=>Math.abs(a.x-x)-Math.abs(b.x-x))[0];
 if(near){const calls=near.sample.report?.calls||[];const bp=near.peak.size_bp;
  const called=calls.some(c=>c.channel===near.tag&&Math.abs(c.size_bp-bp)<0.05);
  const marker=G.markers.find(m=>m.channel===near.tag&&bp>=m.min_bp&&bp<=m.max_bp);
  ctxMenu(e.clientX,e.clientY,[{label:T('addAlleleLabel')+` (${bp.toFixed(2)} bp)`,disabled:called||!marker,
   run:()=>edit(near.sample,{action:'add',marker:marker.name,new:binLabel(marker,bp),peak:{scan:near.peak.scan,size_bp:bp,height:near.peak.height,channel:near.tag}})}])}});
function zoom(f){const [d0,d1]=bpRange();const a=view.x0??d0,b=view.x1??d1,c=(a+b)/2,w=(b-a)*f/2;view.x0=c-w;view.x1=c+w;draw()}

// ---------- lower: genotypes table + QVD ----------
function renderLower(){
 $('lower').style.display=view.table==='none'?'none':'flex';
 const grid=$('gtGrid');grid.replaceChildren();if(!payload)return;
 const t=el('table',{class:'g'});const cols=view.table==='sizing'?[T('colSampleName'),T('colAllele'),T('colSize'),T('colHeight'),T('colArea'),T('colDataPoint')]:[T('colSampleName'),T('colMarker'),T('colAllele')+' 1',T('colAllele')+' 2','GQ'];
 const hr=el('tr');for(const c of cols)hr.append(el('th',{text:c}));t.append(el('thead',{},hr));const tb=el('tbody');
 for(const s of payload.samples){
  if(view.table==='sizing'){for(const c of s.report?.calls||[])tb.append(el('tr',{},el('td',{text:s.name}),el('td',{text:c.label}),el('td',{class:'num',text:c.size_bp.toFixed(2)}),
   el('td',{class:'num',text:String(Math.round(c.height))}),el('td',{class:'num',text:String((s.report.peaks||[]).find(p=>p.id===c.source_peak_id)?.area??'')}),el('td',{class:'num',text:String(Math.round(c.scan))})));continue}
  for(const m of s.markers||[]){const tr=el('tr',{class:view.selMarker===m.marker&&view.selSample===s.id?'sel':''});
   const g=el('td',{class:'ic'});g.innerHTML=pqv(m.gqCat);
   tr.append(el('td',{text:s.name}),el('td',{text:m.marker}),el('td',{text:m.calls[0]?.label||''}),el('td',{text:m.calls[1]?.label||''}),g);
   tr.addEventListener('mousedown',()=>{view.selMarker=m.marker;view.selSample=s.id;renderLower()});tb.append(tr)}}
 t.append(tb);grid.append(t);
 const q=$('qvd');q.replaceChildren(el('h3',{text:T('qvd')}));
 const s=payload.samples.find(x=>x.id===view.selSample);const row=s?.report?.quality?.marker_gq?.markers?.find(m=>m.marker===view.selMarker);
 if(!row)return;
 const sc=row.score||{};q.append(el('p',{text:`GQ = ${sc.gq??'—'}`}));
 const bad=Object.entries(sc.metrics||{}).filter(([,v])=>v!==0&&v!==-2);
 if(!bad.length){q.append(el('p',{text:T('qvdAllMet')}));return}
 q.append(el('p',{text:T('qvdThresholds')}));
 const tt=el('table');tt.append(el('tr',{},...[T('colPqv'),T('colObserved'),T('colThreshold'),T('colWeight')].map(h=>el('th',{text:h}))));
 for(const [k,v] of bad){const thr={allele_number:'max_alleles',absolute_height:'heterozygous_height',high_height:'high_height',ratio:'ratio',broad:'width_bp'}[k];
  tt.append(el('tr',{},el('td',{text:METRIC_PQV[k]||k}),el('td',{text:String(v)}),el('td',{text:thr?String(sc.thresholds?.[thr]??'N/A'):'N/A'}),el('td',{text:String(sc.weights?.[k]??'')})))}
 q.append(tt)}

// ---------- chrome ----------
function build(){
 document.documentElement.lang=I18N.lang;
 document.title=`${T('app')} : ${T('samplesPlot')}`;
 const set=(k,v)=>()=>{view[k]=v;build();draw()};
 const ck=(k,v)=>view[k]===v;
 const dyeItems=DYE_KEYS.map(t=>({label:T('dyes')+': '+DYE_NAMES[t],checked:view.dyes.has(t),run:()=>{view.dyes.has(t)?view.dyes.delete(t):view.dyes.add(t);build();draw()}}));
 buildMenubar($('menubar'),[
  {label:T('file'),items:[{label:T('exportTable'),sc:'Ctrl+E',disabled:true},'-',{label:T('pageSetup'),disabled:true},{label:T('printPreview'),disabled:true},{label:T('print'),sc:'Ctrl+P',run:()=>window.print()},'-',{label:T('closePlot'),run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('undo'),sc:'Ctrl+Z',disabled:true},{label:T('copy'),sc:'Ctrl+C',disabled:true}]},
  {label:T('view'),items:[{label:T('sampleInfo'),disabled:true},{label:T('rawData'),disabled:true},{label:T('eptData'),disabled:true},'-',
   {label:T('language'),sub:[{label:'Русский',checked:I18N.lang==='ru',run:()=>{I18N.set('ru');build();draw()}},{label:'English',checked:I18N.lang==='en',run:()=>{I18N.set('en');build();draw()}}]}]},
  {label:T('plots'),items:[{label:T('zoom'),sub:[{label:T('zoomIn'),sc:'Ctrl+=',run:()=>zoom(.5)},{label:T('zoomOut'),sc:'Ctrl+-',run:()=>zoom(2)},{label:T('fullView'),run:()=>{view.x0=view.x1=null;draw()}}]},
   {label:T('xAxis'),sub:[{label:T('basepairs'),checked:true},{label:T('dataPoints'),disabled:true}]},
   {label:T('yAxis'),sub:[{label:T('scaleInd'),checked:true},{label:T('scaleMaxY'),disabled:true}]},
   {label:T('labels'),sub:[{label:T('noLabels'),checked:ck('labels','none'),run:set('labels','none')},{label:T('hLabels'),checked:ck('labels','h'),run:set('labels','h')},{label:T('vLabels'),checked:ck('labels','v'),run:set('labels','v')}]},
   {label:T('dyes'),sub:dyeItems},'-',
   {label:T('combine'),checked:ck('layout','combine'),run:set('layout','combine')},{label:T('separate'),checked:ck('layout','separate'),run:set('layout','separate')},{label:T('overlay'),checked:ck('layout','overlay'),run:set('layout','overlay')},'-',
   {label:T('showHeader'),checked:view.header,run:set('header',!view.header)},{label:T('markerRange'),checked:view.markerRange,run:set('markerRange',!view.markerRange)},{label:T('showBins'),checked:view.bins,run:set('bins',!view.bins)},'-',
   {label:T('table'),sub:[{label:T('noTable'),checked:ck('table','none'),run:set('table','none')},{label:T('sizingTable'),checked:ck('table','sizing'),run:set('table','sizing')},{label:T('genotypesTable'),checked:ck('table','genotypes'),run:set('table','genotypes')}]}]},
  {label:T('tools'),items:[{label:T('manager'),disabled:true}]},
  {label:T('alleles'),items:[{label:T('editHowTo'),run:()=>message(T('editHint'))},'-',{label:T('history'),run:()=>{try{window.opener.OT.labelEditViewer()}catch(e){}}}]},
  {label:T('help'),items:[{label:T('about'),run:()=>message(T('aboutText'),T('about'))}]}]);
 const t1=$('toolbar1');t1.replaceChildren(tbutton('print',T('print'),()=>window.print()),tbutton('printPreview',T('printPreview'),null,{disabled:true}),tsep(),el('span',{class:'tlabel',text:T('plotSetting')}));
 const ps=el('select',{class:'combo'});ps.append(new Option(T('dataInterpretation'),'di'),new Option(T('sizingData'),'sd'));ps.value=view.table==='sizing'?'sd':'di';
 ps.addEventListener('change',()=>{view.table=ps.value==='sd'?'sizing':'genotypes';build();draw()});
 const pn=el('select',{class:'combo',style:'min-width:44px'});for(let i=1;i<=8;i++)pn.append(new Option(String(i),String(i)));pn.value=String(view.panes);
 pn.addEventListener('change',()=>{view.panes=+pn.value;draw()});
 t1.append(ps,tbutton('plotSettingEditor',T('plotSetting'),null,{disabled:true}),tsep(),el('span',{class:'tlabel',text:T('panes')}),pn,tsep());
 for(const tag of DYE_KEYS){const b=el('button',{class:'tb'+(view.dyes.has(tag)?' on':''),type:'button',title:DYE_NAMES[tag]});
  b.innerHTML=`<svg viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12" fill="${DYE_COLORS[tag]==='#000000'?'#e5d300':DYE_COLORS[tag]}" stroke="#333"/></svg>`;
  b.addEventListener('click',()=>{view.dyes.has(tag)?view.dyes.delete(tag):view.dyes.add(tag);build();draw()});t1.append(b)}
 const tg=(name,title,on,run)=>{const b=tbutton(name,title,run);if(on)b.classList.add('on');return b};
 const t2=$('toolbar2');t2.replaceChildren(
  tg('header',T('showHeader'),view.header,set('header',!view.header)),tg('markerRange',T('markerRange'),view.markerRange,set('markerRange',!view.markerRange)),tsep(),
  tg('noLabels',T('noLabels'),ck('labels','none'),set('labels','none')),tg('hLabels',T('hLabels'),ck('labels','h'),set('labels','h')),tg('vLabels',T('vLabels'),ck('labels','v'),set('labels','v')),
  tg('bins',T('showBins'),view.bins,set('bins',!view.bins)),tsep(),tbutton('fullView',T('fullView'),()=>{view.x0=view.x1=null;draw()}),tsep(),
  tg('combine',T('combine'),ck('layout','combine'),set('layout','combine')),tg('separate',T('separate'),ck('layout','separate'),set('layout','separate')),tg('overlay',T('overlay'),ck('layout','overlay'),set('layout','overlay')),tsep(),
  tg('noTable',T('noTable'),ck('table','none'),set('table','none')),tg('sizingTable',T('sizingTable'),ck('table','sizing'),set('table','sizing')),tg('genotypesTable',T('genotypesTable'),ck('table','genotypes'),set('table','genotypes')));
 const ch=$('colhead');ch.replaceChildren(el('div',{class:'c',style:'width:250px',text:T('colSampleFile')}),el('div',{class:'c',style:'width:150px',text:T('colSampleName')}),
  ...PQV_KEYS.map(k=>el('div',{class:'c',style:'width:46px',text:k})));ch.style.display=view.header?'flex':'none';
}
document.addEventListener('keydown',e=>{if(!(e.ctrlKey||e.metaKey))return;if(e.key==='='||e.key==='+'){e.preventDefault();zoom(.5)}if(e.key==='-'){e.preventDefault();zoom(2)}});
window.addEventListener('resize',()=>draw());
// Follow the project window's selection (Project Options: update plots when the selection changes).
function refresh(){const before=JSON.stringify(payload?.samples?.map(s=>s.id));let next=null;try{next=window.opener?.OT?.plotPayload()}catch(e){}
 if(next&&JSON.stringify(next.samples.map(s=>s.id))!==before&&next.samples.length){payload=next;build();draw()}
 else if(next){payload.project=next.project;payload.printComment=next.printComment;printHead()}}
window.otRefresh=refresh;
window.addEventListener('focus',refresh);
function printHead(){const h=document.getElementById('printHead');if(!h)return;
 h.textContent=[payload?.project||'',payload?.printComment||''].filter(Boolean).join(' — ')}
window.addEventListener('beforeprint',printHead);
load();
})();
