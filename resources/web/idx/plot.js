/* Hot keys follow the physical key (works with a Russian layout: Ctrl+В is Ctrl+D). */
var hotKey=e=>/^Key[A-Z]$/.test(e.code||'')?e.code.slice(3).toLowerCase():String(e.key||'').toLowerCase();
// OpenTrace Samples Plot window.
(function(){
const $=id=>document.getElementById(id);
const DYE_COLORS={DATA1:'#0000ff',DATA2:'#00a000',DATA3:'#000000',DATA4:'#ff0000',DATA105:'#ff8000',DATA106:'#b000d0'};
// DATA105 = 5th dye (orange LIZ in 5- and 6-dye data), DATA106 = 6th dye (purple SID of 6-dye kits); panes in the order B G Y R P O
const DYE_KEYS=['DATA1','DATA2','DATA3','DATA4','DATA106','DATA105'];
const DYE_NAMES={DATA1:'blue',DATA2:'green',DATA3:'yellow',DATA4:'red',DATA105:'orange',DATA106:'purple'};
// marker header colours of ID-X plots (green / yellow-orange / red GQ; grey = edited or no GQ)
const GQ_FILL={pass:'#009900',check:'#ff9900',low:'#cc0000',na:'#7f7f7f'};
const BIN_FILL='#d8d8d8',VBIN_FILL='#fecbcb';
// allele-calling window and bin offsets of a sample's marker (bins shift by the ladder offsets of its run)
function markerCalling(s,name){const m=s.report?.allele_calling?.markers;if(!m)return null;return Array.isArray(m)?m.find(x=>x.name===name):m[name]||Object.values(m).find(x=>x.name===name)}
const sampleEdited=s=>(s.markers||[]).some(r=>r.edited);
const PQV_KEYS=['SOS','SQ','SSPK','MIX','OMR','CGQ'];
const METRIC_PQV={allele_number:'AN',absolute_height:'LPH',high_height:'MPH',spike:'SPK',ratio:'PHR',broad:'BD',offscale:'OS',overlap:'OVL',binning:'BIN'};

let payload=null;
const view={panes:2,layout:'separate',labels:'h',bins:true,header:true,markerRange:true,table:'genotypes',dyes:new Set(DYE_KEYS),
 x0:null,x1:null,selMarker:null,selSample:null,xAxis:'bp',yAxis:'individual',yValue:1000,peakPositions:false,offscale:true,toolbar:true,
 alleleChanges:true,sizeStdLabels:false,controlsTop:false,laddersTop:false,customColors:false};
// ---------- plot settings (Plot Settings Editor) ----------
const PS_ACTIVE='opentrace.plotSettings.active';
const NAME_DYE={blue:'DATA1',green:'DATA2',yellow:'DATA3',red:'DATA4',purple:'DATA106',orange:'DATA105'};
let setting=null;
function settingNames(){return PlotSettings.list().map(r=>r.setting.name).filter(n=>!window.Sec||!Sec.items||Sec.items.can('plots',n,'read'))}
function activeName(){let n='';try{n=localStorage.getItem(PS_ACTIVE)||''}catch(e){}const names=settingNames();return names.includes(n)?n:(names.includes('Data Interpretation')?'Data Interpretation':names[0])}
function applySetting(name){const keep=view.retainX?{x0:view.x0,x1:view.x1,userZoom:view.userZoom}:null;setting=PlotSettings.get(name)||PlotSettings.factory()[0];try{localStorage.setItem(PS_ACTIVE,setting.name)}catch(e){}
 const D=setting.display.persistence==='last'&&setting.lastView?{...setting.display,...setting.lastView}:setting.display;
 Object.assign(view,{panes:D.panes,layout:D.layout,labels:D.labels,bins:D.bins,header:D.header,markerRange:D.markerRange,table:D.table==='lev'?'none':D.table,
  xAxis:D.xAxis,yAxis:D.yAxis,yValue:D.yValue,peakPositions:D.peakPositions,offscale:D.offscale,toolbar:D.toolbar,alleleChanges:D.alleleChanges,
  sizeStdLabels:D.sizeStdLabels,controlsTop:D.controlsTop,laddersTop:D.laddersTop,markerIndicator:D.markerIndicator!==false,x0:null,x1:null});
 view.dyes=new Set(Object.entries(D.dyes).filter(([,on])=>on).map(([d])=>NAME_DYE[d]));
 view.userZoom=false;if(!D.allDyes&&D.xAxis==='bp'&&D.end>D.start){view.x0=D.start;view.x1=D.end}
 if(keep)Object.assign(view,keep);
 if(D.table==='lev')setTimeout(()=>{try{window.opener.OT.labelEditViewer()}catch(e){}},0)}
// "Use the display settings last used for this plot": the interactive state is kept when the window closes.
window.addEventListener('beforeunload',()=>{if(!setting||setting.display.persistence!=='last')return;
 const s=PlotSettings.get(setting.name);if(!s)return;const inv=Object.fromEntries(Object.entries(NAME_DYE).map(([k,v])=>[v,k]));
 s.lastView={panes:view.panes,layout:view.layout,labels:view.labels,bins:view.bins,header:view.header,markerRange:view.markerRange,table:view.table,xAxis:view.xAxis,
  yAxis:view.yAxis,dyes:Object.fromEntries(Object.keys(NAME_DYE).map(d=>[d,view.dyes.has(NAME_DYE[d])]))};
 try{PlotSettings.put(s,s.name)}catch(e){}});
function editSetting(){if(!setting)return;const ro=window.Sec&&Sec.items&&!Sec.items.can('plots',setting.name,'update');
 PlotSettings.openEditor(setting,{restrict:ro?'read':null,onSave:(v,prev)=>{PlotSettings.put(v,prev);if(window.Sec&&Sec.items)Sec.items.rename('plots',prev,v.name);applySetting(v.name);build();draw()}})}

function load(){
 applySetting(activeName());
 try{payload=window.opener&&window.opener.OT?window.opener.OT.plotPayload():null}catch(e){payload=null}
 if(payload?.lang)I18N.set(payload.lang);
 build();draw();printHead()}

// ---------- scan <-> bp ----------
function curveOf(a){const anchors=a?.sizing?.anchors;if(!anchors||anchors.length<2)return null;return [...anchors].sort((p,q)=>p[0]-q[0])}
function bpToScan(c,b){if(!c)return b;let i=1;while(i<c.length-1&&b>c[i][1])i++;const [s0,b0]=c[i-1],[s1,b1]=c[i];return s0+(b-b0)*(s1-s0)/(b1-b0)}
function domain(){if(view.xAxis!=='dp')return bpRange();let n=0;for(const s of payload?.samples||[])for(const c of s.analysis?.channels||[])n=Math.max(n,(c.values||[]).length);return [0,n||10000]}
function scanToBp(c,s){if(!c)return s;let i=1;while(i<c.length-1&&s>c[i][0])i++;const [s0,b0]=c[i-1],[s1,b1]=c[i];return b0+(s-s0)*(b1-b0)/(s1-s0)}
function bpRange(){const ms=payload?.kit?.markers||[];
 if(ms.length)return [Math.min(...ms.map(m=>m.min_bp))-10,Math.max(...ms.map(m=>m.max_bp))+10];
 for(const s of payload?.samples||[]){const c=curveOf(s.analysis);if(c)return [c[0][1],c[c.length-1][1]]}
 return [0,500]}

// ---------- panes ----------
const isGeno=()=>payload?.mode==='genotypes';
const GENO_PQV=['AN','BIN','PHR','LPH','MPH','BD','SPK','OS','OVL','ACC','CC','GQ'];
function paneList(){
 const out=[];
 // Genotypes plot: one marker per pane, in the order the genotypes were selected
 if(isGeno()){for(const key of payload.genoSel||[]){const [sid,name]=[+key.split('|')[0],key.slice(key.indexOf('|')+1)];const s=payload.samples.find(x=>x.id===sid);if(!s)continue;
   const m=((s.kit||payload.kit)?.markers||[]).find(x=>x.name===name);if(!m)continue;out.push({sample:s,dyes:[m.channel],marker:m})}
  return out}
 // Bring Controls / Ladders to Top: those samples come first
 const rank=s=>(view.laddersTop&&s.type==='ladder')||(view.controlsTop&&(s.type==='positive'||s.type==='negative'))?0:1;
 for(const s of [...(payload?.samples||[])].sort((a,b)=>rank(a)-rank(b))){
  const tags=(s.analysis?.channels||[]).filter(c=>c.group!=='auxiliary'&&DYE_KEYS.includes(c.tag)).map(c=>c.tag);
  // the size-standard dye is shown only when selected in the setting (e.g. a sizing review setting)
  const shown=(tags.length?tags:DYE_KEYS.slice(0,4)).filter(t=>view.dyes.has(t));
  if(view.layout==='separate')for(const t of shown)out.push({sample:s,dyes:[t]});
  else out.push({sample:s,dyes:shown})}
 if(view.layout==='overlay')return out.length?[{overlay:true,items:out}]:[];
 return out}

function draw(){
 const area=$('plotarea');paintObserver?.disconnect();area.replaceChildren();
 if(!payload||!payload.samples.length){area.append(el('div',{class:'empty',text:T('selectSamplesFirst')}));renderLower();return}
 const panes=paneList();
 // multi-line labels (Labels tab) get extra room so the trace keeps its height
 const LSd=setting?.labels;const nl=LSd?Math.max(1,...['assigned','custom','ladder','artifact'].map(k=>{const i=LSd[k].indexOf('none');return i<0?4:Math.max(1,i)})):1;
 const extra=view.labels==='none'?0:2*(nl-1)*((LSd?.size||11)+3);
 const h=Math.max(150+extra,Math.floor((area.clientHeight-2)/Math.max(1,view.panes)));
 const [d0,d1]=domain();const x0=view.x0??d0,x1=view.x1??d1;
 view.yGlobal=view.yAxis==='max'?Math.max(1,...panes.flatMap(p=>(p.overlay?p.items:[p]).flatMap(it=>{let a=x0,b=x1;if(p.marker&&!view.userZoom){const mg=setting?.display?.markerMargin??5;a=p.marker.min_bp-mg;b=p.marker.max_bp+mg;
  if(view.xAxis==='dp'){const c=curveOf(it.sample.analysis);a=bpToScan(c,a);b=bpToScan(c,b)}}return traceData(it,a,b).map(t=>t.max)}))):null;
 for(const p of panes){
  const items=p.overlay?p.items:[p];const s=items[0].sample;
  const pane=el('div',{class:'pane'});
  if(view.header&&p.marker){const head=el('div',{class:'phead'});const G=setting?.genotypeHeader||{};const on=k=>G[k]!==false;
   const row=(s.markers||[]).find(r=>r.marker===p.marker.name);
   if(on('sampleFileName'))head.append(el('div',{class:'c',style:'width:250px',text:s.file}));
   if(on('sampleName'))head.append(el('div',{class:'c',style:'width:150px',text:s.name}));
   if(on('panel'))head.append(el('div',{class:'c',style:'width:150px',text:row?.panel||s.kit?.name||''}));
   if(on('marker'))head.append(el('div',{class:'c',style:'width:100px',text:p.marker.name}));
   for(const k of GENO_PQV.filter(on)){const c=el('div',{class:'c',style:'width:46px;justify-content:center'});c.innerHTML=pqv(row?.flags?.[k]);head.append(c)}
   head._sample=s;pane.append(head)}
  else if(view.header){
   const head=el('div',{class:'phead'});
   const H=setting?.sampleHeader||{};const on=k=>H[k]!==false;
   if(on('sampleFileName'))head.append(el('div',{class:'c',style:'width:250px',text:p.overlay?'':s.file}));
   if(on('sampleName'))head.append(el('div',{class:'c',style:'width:150px',text:p.overlay?'':s.name}));
   if(on('panel'))head.append(el('div',{class:'c',style:'width:150px',text:p.overlay?'':(s.kit?.panel||s.kit?.name||'')}));
   for(const k of PQV_KEYS.filter(on)){const c=el('div',{class:'c',style:'width:46px;justify-content:center'});c.innerHTML=p.overlay?'':pqv(s.pqv?.[k],k!=='SQ'&&sampleEdited(s));head.append(c)}
   if(!p.overlay){const lab=el('label',{class:'c',style:'gap:4px'});lab.append(el('input',{type:'checkbox'}),T('markSample'));head.append(lab);head._sample=s;head.title=T('headerMenuHint')}
   pane.append(head)}
  const canvas=el('canvas');canvas.height=h;canvas._sid=s.id;canvas._dyes=items.flatMap(i=>i.dyes);pane.append(canvas);area.append(pane);
  // the Genotypes plot shows the marker size range plus the Marker Margin of the plot setting
  let px0=x0,px1=x1;if(p.marker&&!view.userZoom){const mg=setting?.display?.markerMargin??5,c=curveOf(s.analysis);
   px0=p.marker.min_bp-mg;px1=p.marker.max_bp+mg;if(view.xAxis==='dp'){px0=bpToScan(c,px0);px1=bpToScan(c,px1)}}
  lazyPaint(area,canvas,()=>paint(canvas,items,px0,px1,p.marker))}
 renderLower()}

// A pane is painted when it scrolls near the visible part of the plot area, not all at once: 96 samples in separate
// dye panes are 384 canvases, and every selection change redraws. Same pixels as eager painting; printing paints
// its own canvases synchronously.
let paintObserver=null;
function lazyPaint(area,canvas,fn){
 if(!('IntersectionObserver' in window)){requestAnimationFrame(fn);return}
 if(!paintObserver||paintObserver.root!==area)paintObserver=new IntersectionObserver(es=>{for(const e of es)if(e.isIntersecting){paintObserver.unobserve(e.target);const f=e.target._lazyPaint;e.target._lazyPaint=null;if(f)f()}},{root:area,rootMargin:'100% 0px'});
 canvas._lazyPaint=fn;canvas._paint=fn;paintObserver.observe(canvas)}
// Selection changes repaint the panes already drawn in place (no new canvases): fast with hundreds of panes, and the
// panes keep their geometry, so arrow keys pressed in quick succession always find the selected peak.
function repaint(){for(const cv of document.querySelectorAll('#plotarea canvas'))if(cv._geom&&cv._paint)cv._paint();renderLower()}

function paint(cv,items,x0,x1,onlyMarker){
 const dpr=window.devicePixelRatio||1,W=cv.clientWidth,H=cv._h??(cv._h=cv.height);cv.width=W*dpr;cv.height=H*dpr;cv.style.height=H+'px';
 const g=cv.getContext('2d');g.scale(dpr,dpr);g.fillStyle='#fff';g.fillRect(0,0,W,H);g.font='11px Tahoma,Arial';
 const LS=setting?.labels||PlotSettings.base('').labels;const fs=LS.size||11;const lineH=fs+5;
 const maxLines=Math.max(1,...['assigned','custom','ladder','artifact'].map(k=>{const i=LS[k].indexOf('none');return i<0?4:Math.max(1,i)}));const labelH=maxLines*lineH+3;
 const L=56,R=14,top=view.markerRange?40:18,bottom=H-(view.labels==='none'?16:10+2*(labelH+2));
 const s=items[0].sample;const cur=curveOf(s.analysis);const dp=view.xAxis==='dp';
 const pos=(bp,scan)=>dp?(scan??bpToScan(cur,bp)):bp;   // plot coordinate of a peak / bin edge
 const Xc=v=>L+(v-x0)/(x1-x0)*(W-L-R);const X=bp=>Xc(dp?bpToScan(cur,bp):bp);
 const markers=((s.kit||payload.kit)?.markers||[]).filter(m=>!onlyMarker||m.name===onlyMarker.name);
 // Click areas of marker headers per canvas (the marker objects belong to the opener's kit: never written to).
 const markerHits=[];
 const dyes=items.flatMap(i=>i.dyes);
 // marker headers + bins
 for(const m of markers.filter(m=>dyes.includes(m.channel))){
  const row=(s.markers||[]).find(r=>r.marker===m.name);const xa=X(m.min_bp),xb=X(m.max_bp);
  // marker header colour = GQ; grey when its allele labels were edited
  if(view.markerRange){const col=row?.edited?GQ_FILL.na:GQ_FILL[row?.gqCat||'na'];const hy=3,hh=16;
   // ID-X 1.4 prints filled headers like the screen; the ID-X 1.6 help describes unfilled headers outlined in colour (Page Setup choice)
   if(view.printing&&view.headerStyle==='outline'){g.fillStyle='#fff';g.fillRect(xa,hy,xb-xa,hh);g.lineWidth=2;g.strokeStyle=col;g.strokeRect(xa+1,hy+1,xb-xa-2,hh-2);g.lineWidth=1}
   else{g.fillStyle=col;g.fillRect(xa,hy,xb-xa,hh);g.strokeStyle='#000';g.strokeRect(xa+.5,hy+.5,xb-xa-1,hh-1)}
   g.save();g.font='12px Arial,Helvetica,sans-serif';g.textAlign='center';g.fillStyle='#000';const name=(row?.pli?'** ':'')+m.name;
   g.fillText(g.measureText(name).width<=xb-xa-4?name:'...',(xa+xb)/2,hy+hh-4);g.restore();
   markerHits.push({m,x0:xa,x1:xb,y0:hy,y1:hy+hh})}
  if(view.bins){const off=markerCalling(s,m.name)?.offsets?.by_bin||{};
   for(const b of m.bins||[]){const d=off[b.label]?.offset_bp||0;const ba=X(b.center_bp-d-(b.left_bp??.5)),bb=X(b.center_bp-d+(b.right_bp??.5));
    g.fillStyle=b.virtual?VBIN_FILL:BIN_FILL;g.fillRect(ba,top,Math.max(1,bb-ba),bottom-top)}}}
 // axes
 g.strokeStyle='#000';g.strokeRect(L+.5,top+.5,W-L-R-1,bottom-top);
 const step=niceStep((x1-x0)/7);g.fillStyle='#000';g.textAlign='center';g.font='12px Arial,Helvetica,sans-serif';
 for(let v=Math.ceil(x0/step)*step;v<=x1;v+=step){const x=Xc(v);g.fillText(String(+v.toFixed(1)),x,top-5);g.beginPath();g.moveTo(x,top-3);g.lineTo(x,top+4);g.stroke()}
 for(let v=Math.ceil(x0/step*2)*step/2;v<=x1;v+=step/2){const x=Xc(v);g.beginPath();g.moveTo(x,top);g.lineTo(x,top+2);g.stroke()}
 g.font='11px Tahoma,Arial';
 // traces
 let ymax=1;const traces=[];
 for(const it of items)for(const t of traceData(it,x0,x1)){ymax=Math.max(ymax,t.max);traces.push(t)}
 if(!traces.length){g.fillStyle='#777';g.textAlign='center';g.fillText(T('noValues'),W/2,(top+bottom)/2);return}
 // Y axis: scale individually, to the maximum Y of all panes, or to the entered value
 ymax=view.yAxis==='entered'?view.yValue:view.yAxis==='max'&&view.yGlobal?view.yGlobal*1.08:ymax*1.08;const Y=v=>bottom-(Math.min(v,ymax*1.2)/ymax)*(bottom-top);
 g.textAlign='right';g.font='12px Arial,Helvetica,sans-serif';const ys=niceStep(ymax/2.2);
 for(let v=0;v<=ymax;v+=ys){const y=Y(v);g.fillText(String(Math.round(v)),L-6,y+4);g.beginPath();g.moveTo(L-5,y);g.lineTo(L,y);g.stroke()}
 for(let v=ys/2;v<=ymax;v+=ys){const y=Y(v);g.beginPath();g.moveTo(L-3,y);g.lineTo(L,y);g.stroke()}
 g.font='11px Tahoma,Arial';
 g.save();g.beginPath();g.rect(L,top,W-L-R,bottom-top);g.clip();
 // off-scale data: pink band (Display Settings "Off-scale data")
 if(view.offscale)for(const t of traces)for(const pk of t.peaks)if(pk.offscaled&&pk.size_bp!=null){const a=Xc(pos(pk.size_bp,pk.begin??pk.scan-3)),b=Xc(pos(pk.size_bp,pk.end??pk.scan+3));
  const xa=dp?a:X(pk.size_bp)-3,xb=dp?b:X(pk.size_bp)+3;g.fillStyle='rgba(255,105,180,.3)';g.fillRect(Math.min(xa,xb),top,Math.max(2,Math.abs(xb-xa)),bottom-top)}
 traces.forEach((t,ti)=>{g.strokeStyle=view.customColors&&setting?setting.display.customColors[ti%setting.display.customColors.length]:(DYE_COLORS[t.tag]||'#333');
  g.lineWidth=1;g.beginPath();t.pts.forEach(([x,v],i)=>i?g.lineTo(Xc(x),Y(v)):g.moveTo(Xc(x),Y(v)));g.stroke()});
 // marker indicators: red triangles at the start and end of each marker range after bin offsetting
 if(view.markerIndicator&&!dp)for(const m of markers.filter(m=>dyes.includes(m.channel))){const w=markerCalling(s,m.name)?.window_bp||[m.min_bp,m.max_bp];
  for(const v of w){const x=X(v);if(x<L-1||x>W-R+1)continue;g.fillStyle=view.partial?.has(s.id+'|'+m.name)?'#ff9900':'#ff0000';g.beginPath();g.moveTo(x-4,bottom);g.lineTo(x,bottom-10);g.lineTo(x+4,bottom);g.closePath();g.fill()}}
 // peak position indicators: a tick under each detected peak
 if(view.peakPositions)for(const t of traces)for(const pk of t.peaks){if(pk.size_bp==null&&!dp)continue;const x=Xc(pos(pk.size_bp,pk.scan));if(x<L||x>W-R)continue;
  g.strokeStyle=DYE_COLORS[t.tag]||'#333';g.beginPath();g.moveTo(x,bottom-1);g.lineTo(x,bottom-7);g.stroke()}
 // size-standard labels on the reference dye
 if(view.sizeStdLabels){g.textAlign='center';g.font=`${fs}px ${LS.font||'Tahoma'}`;for(const t of traces){if(t.tag!==t.sample.analysis?.sizing?.reference_channel)continue;
  for(const m of t.sample.analysis?.sizing?.matched||[]){const mbp=m.expected_bp??m.bp;if(m.scan==null||mbp==null)continue;const v=dp?m.scan:mbp;if(v<x0||v>x1)continue;
   g.fillStyle='#000';g.fillText(String(+(+mbp).toFixed(1)),Xc(v),Math.max(top+fs,Y((m.height||0)*(t.fk||1))-4))}}}
 g.restore();
 // allele labels in boxes under the axis
 const labelBoxes=[];let noRoom=false;
 if(view.labels!=='none'){g.textAlign='center';const placed=[];
  g.font=`${fs+2}px ${PlotSettings.cssFont(LS.font)}`;
  for(const t of traces){const calls=(t.sample.report?.calls||[]).filter(c=>c.channel===t.tag&&(dp?c.scan>=x0&&c.scan<=x1:c.size_bp>=x0&&c.size_bp<=x1));
   const dyeCol=DYE_COLORS[t.tag]==='#000000'?'#7a6a00':(DYE_COLORS[t.tag]||'#000');
   for(const c of calls){const x=Xc(pos(c.size_bp,c.scan));const lines=labelLines(c,t.sample);
    const w=Math.max(18,...lines.map(l=>g.measureText(l).width+8));let row=0;while(placed.some(p=>p.row===row&&Math.abs(p.x-x)<(p.w+w)/2+2))row++;if(row>1){noRoom=true;continue}placed.push({x,row,w});
    const y=bottom+6+row*(labelH+2);const h=lines.length*lineH+2;
    const border=(c.flags||[]).length?'#e040a0':LS.color==='bw'?'#000':dyeCol;g.strokeStyle=border;g.fillStyle='#fff';g.fillRect(x-w/2,y,w,h);g.strokeRect(x-w/2+.5,y+.5,w-1,h-1);
    g.fillStyle=view.alleleChanges&&c.user_edited?'#b00060':LS.color==='dyeAll'?dyeCol:'#000';lines.forEach((l,i)=>g.fillText(l,x,y+lineH*(i+1)-2));
    g.beginPath();g.moveTo(x,bottom);g.lineTo(x,y);g.strokeStyle=LS.color==='bw'?'#999':dyeCol;g.stroke();
    labelBoxes.push({x0:x-w/2,x1:x+w/2,y0:y,y1:y+h,call:c,sample:t.sample})}
   for(const o of (t.sample.omr||[]).filter(o=>{const v=pos(o.size_bp,o.scan);return o.channel===t.tag&&v>=x0&&v<=x1})){const x=X(o.size_bp);let row=0;while(placed.some(p=>p.row===row&&Math.abs(p.x-x)<30))row++;if(row>1){noRoom=true;continue}placed.push({x,row});
    const y=bottom+6+row*16,w=g.measureText('OMR').width+8;g.strokeStyle='#d0700a';g.fillStyle='#fff3e0';g.fillRect(x-w/2,y,w,14);g.strokeRect(x-w/2+.5,y+.5,w-1,13);
    g.fillStyle='#a04f00';g.fillText('OMR',x,y+11);g.beginPath();g.moveTo(x,bottom);g.lineTo(x,y);g.strokeStyle='#d0700a';g.stroke()}
   // artifact labels (user-assigned and automatic SPK) have a pink border
   const arts=[...(t.sample.report?.artifacts||[]).map(a=>({...a,user:true})),...(t.sample.spk||[]).map(a=>({...a,label:'SPK'}))];
   for(const a of arts.filter(a=>{const v=pos(a.size_bp,a.scan);return a.channel===t.tag&&v>=x0&&v<=x1})){const x=X(a.size_bp);let row=0;while(placed.some(p=>p.row===row&&Math.abs(p.x-x)<30))row++;if(row>1){noRoom=true;continue}placed.push({x,row});
    const y=bottom+6+row*16,w=Math.max(18,g.measureText(a.label).width+8);g.strokeStyle='#e040a0';g.fillStyle='#fff';g.fillRect(x-w/2,y,w,14);g.lineWidth=1.5;g.strokeRect(x-w/2+.5,y+.5,w-1,13);g.lineWidth=1;
    g.fillStyle='#b0006a';g.fillText(a.label,x,y+11);g.beginPath();g.moveTo(x,bottom);g.lineTo(x,y);g.strokeStyle='#e040a0';g.stroke();
    if(a.user)labelBoxes.push({x0:x-w/2,x1:x+w/2,y0:y,y1:y+14,artifact:a,sample:t.sample})}}}
 // ID-X "No room for labels": labels that do not fit in the label rows are not drawn
 if(noRoom){g.save();g.font='italic 11px Tahoma,Arial';g.textAlign='right';const tw=g.measureText(T('noRoomLabels')).width;g.fillStyle='rgba(255,255,255,.9)';g.fillRect(W-R-tw-8,H-16,tw+8,15);g.fillStyle='#b00000';g.fillText(T('noRoomLabels'),W-R-4,H-4);g.restore()}
 const peakPts=[];for(const t of traces)for(const pk of t.peaks){if(pk.size_bp==null)continue;const v=pos(pk.size_bp,pk.scan);if(v>=x0&&v<=x1)peakPts.push({x:Xc(v),y:Y(pk.height*(t.fk||1)),peak:pk,tag:t.tag,sample:t.sample})}
 // selected peaks (ID-X: "the peak fills in when it is selected")
 for(const pp of peakPts){if(!view.selPeaks.has(peakKey(pp.sample,pp.tag,pp.peak)))continue;const col=DYE_COLORS[pp.tag]==='#000000'?'#555':(DYE_COLORS[pp.tag]||'#333');
  // the area of the selected peak (from its start to its end scan) is filled in the dye colour, as in ID-X
  const t=traces.find(t=>t.tag===pp.tag&&t.sample===pp.sample),pk=pp.peak,c2=curveOf(pp.sample.analysis);
  const lo=pk.begin!=null?(dp?pk.begin:scanToBp(c2,pk.begin)):null,hi=pk.end!=null?(dp?pk.end:scanToBp(c2,pk.end)):null;
  const pts=t&&lo!=null&&hi!=null&&hi>lo?t.pts.filter(([x])=>x>=lo&&x<=hi):[];
  g.save();g.beginPath();g.rect(L,top,W-L-R,bottom-top);g.clip();g.fillStyle=col;
  if(pts.length>1){g.globalAlpha=.8;g.beginPath();g.moveTo(Xc(pts[0][0]),bottom);for(const [x,v] of pts)g.lineTo(Xc(x),Y(v));g.lineTo(Xc(pts[pts.length-1][0]),bottom);g.closePath();g.fill()}
  else{g.globalAlpha=.35;g.beginPath();g.moveTo(pp.x-4,bottom);g.lineTo(pp.x,pp.y);g.lineTo(pp.x+4,bottom);g.closePath();g.fill()}
  g.restore();
  g.fillStyle=col;g.beginPath();g.arc(pp.x,pp.y,4,0,7);g.fill();g.strokeStyle='#000';g.stroke()}
 if(view.legend){const names=[...new Set(dyes)];g.font='10px Tahoma,Arial';g.textAlign='left';let ly=top+4;
  for(const t of names){const txt=`${DYE_NAMES[t]||t}: 0–${Math.round(ymax)}`;const w=g.measureText(txt).width+18;const lx=W-R-w-4;
   g.fillStyle='rgba(255,255,255,.85)';g.fillRect(lx-2,ly-2,w+2,14);g.fillStyle=DYE_COLORS[t]==='#000000'?'#e5d300':(DYE_COLORS[t]||'#333');g.fillRect(lx,ly,10,10);g.strokeStyle='#333';g.strokeRect(lx,ly,10,10);
   g.fillStyle='#000';g.fillText(txt,lx+14,ly+9);ly+=15}}
 cv._geom={X,x0,x1,L,R,W,top,bottom,ymax,dp,curve:cur,noRoom,markers:markers.filter(m=>dyes.includes(m.channel)),markerHits,sample:s,labelBoxes,peakPts};
}
// trace points of one pane item in the current coordinate (bp or data points)
const dyeFactor=tag=>{const n=Object.keys(NAME_DYE).find(k=>NAME_DYE[k]===tag);const v=n&&setting?.display?.dyeScales?.[n];return v>0?v:1};
function traceData(it,x0,x1){const a=it.sample.analysis,c=curveOf(a),out=[];
 for(const tag of it.dyes){const ch=(a?.channels||[]).find(q=>q.tag===tag);const values=ch?.analyzed_values||ch?.values;if(!values)continue;
  const pts=[];let max=1;const fk=dyeFactor(tag);values.forEach((v,i)=>{const x=view.xAxis==='dp'?i:scanToBp(c,i);if(x>=x0&&x<=x1){pts.push([x,v*fk]);if(v*fk>max)max=v*fk}});
  out.push({tag,pts,max,peaks:ch.peaks||[],sample:it.sample,fk})}
 return out}
// label lines per the Labels tab: up to four values, stopping at the first "None"
const PREFIX={alleleCall:'al',size:'sz',height:'ht',area:'ar',dataPoint:'dp',aeReason:'ae'};
function labelLines(c,s){const LS=setting?.labels||PlotSettings.base('').labels;
 const kind=s.type==='ladder'?'ladder':c.user_edited?'custom':'assigned';const out=[];
 for(const k of LS[kind]){if(k==='none')break;let v='';
  if(k==='alleleCall')v=String(c.label);else if(k==='size')v=c.size_bp!=null?c.size_bp.toFixed(2):'';else if(k==='height')v=String(Math.round(c.height??0));
  else if(k==='area')v=String((s.report?.peaks||[]).find(p=>p.id===c.source_peak_id)?.area??'');else if(k==='dataPoint')v=String(Math.round(c.scan??0));
  else if(k==='aeReason')v=kind==='custom'?(s.editReasons?.[c.id]||''):'';
  out.push(LS.prefixes?`${PREFIX[k]} ${v}`:v)}
 if(!out.length)out.push(String(c.label));
 if(LS.editType&&c.user_edited)out[0]+=(c.flags||[]).includes('user_added')?' (added)':' (changed)';
 if(LS.pqvTrigger){const tr=triggers(c,s);if(tr.length)out[0]+=' '+tr.join(',')}
 return out}
// PQV trigger peaks (LPH, MPH, BD, OS) from the marker GQ details
function triggers(c,s){const row=(s.report?.quality?.marker_gq?.markers||[]).find(m=>m.marker===c.marker);const sc=row?.score||{},m=sc.metrics||{},th=sc.thresholds||{};
 const pk=(s.report?.peaks||[]).find(p=>p.id===c.source_peak_id)||{};const out=[];
 if(m.absolute_height&&m.absolute_height!==-2&&th.heterozygous_height!=null&&c.height<th.heterozygous_height)out.push('LPH');
 if(m.high_height&&m.high_height!==-2&&th.high_height!=null&&c.height>th.high_height)out.push('MPH');
 if(m.broad&&m.broad!==-2&&th.width_bp!=null&&(pk.morphology_bp?.fwhm_bp??0)>th.width_bp)out.push('BD');
 if(pk.offscaled)out.push('OS');return out}
function niceStep(raw){const p=Math.pow(10,Math.floor(Math.log10(raw||1)));const n=raw/p;return (n<1.5?1:n<3.5?2:n<7.5?5:10)*p}

// ---------- interaction ----------
let drag=null;
const peakKey=(s,tag,pk)=>`${s.id}|${tag}|${pk.size_bp.toFixed(3)}`;
window.__otPeakKey=peakKey;
view.selPeaks=view.selPeaks||new Map();window.__otView=view;
const nearPeak=(G,x)=>G.peakPts.filter(p=>Math.abs(p.x-x)<=6).sort((a,b)=>Math.abs(a.x-x)-Math.abs(b.x-x))[0];
// Zones as in ID-X EfChart: the strip above the plot is the X axis, the strip left of it the Y axis (own zoom, double click and menu).
const zoneAt=(G,x,y)=>y<G.top?'x':(x<G.L&&y<=G.bottom?'y':(x>=G.L&&x<=G.W-G.R&&y<=G.bottom?'plot':null));
let band=null;
function bandShow(cv,d,x,y){if(!band){band=el('div',{style:'position:fixed;border:1px dashed #000;background:rgba(0,0,0,.06);pointer-events:none;z-index:50'});document.body.append(band)}
 const r=cv.getBoundingClientRect(),G=cv._geom;let ax=d.x0,bx=x,ay=d.y0,by=y;
 if(d.mode==='x'){ay=G.top-14;by=G.bottom}else if(d.mode==='y'){ax=G.L-30;bx=G.W-G.R;ay=d.y0;by=y}else if(d.mode==='h'){ax=G.L;bx=G.W-G.R;by=G.bottom}
 const l=Math.min(ax,bx),t=Math.min(ay,by);Object.assign(band.style,{left:r.left+l+'px',top:r.top+t+'px',width:Math.abs(bx-ax)+'px',height:Math.abs(by-ay)+'px'})}
const bandHide=()=>{band?.remove();band=null};
document.addEventListener('mousedown',e=>{const cv=e.target.closest('canvas');if(!cv?._geom)return;const r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top,G=cv._geom;
 const m=(G.markerHits||[]).find(h=>x>=h.x0&&x<=h.x1&&y>=h.y0&&y<=h.y1)?.m;
 if(m){view.selMarker=m.name;view.selSample=G.sample.id;renderLower();return}
 if(e.button!==0)return;
 // a click on an allele / artifact label selects its peak (or, with single-click editing, deletes the label)
 const box=(G.labelBoxes||[]).find(b=>x>=b.x0&&x<=b.x1&&y>=b.y0&&y<=b.y1);
 if(box){e.preventDefault();const lab=box.call||box.artifact,multi=e.ctrlKey||e.metaKey||e.shiftKey;
  if(singleClickOn()&&box.sample.editable&&!multi){deleteLabelsOf([{s:box.sample,label:box.call?{call:box.call}:{artifact:box.artifact}}]);return}
  const pp=G.peakPts.find(p=>p.sample===box.sample&&p.tag===lab.channel&&Math.abs(p.peak.size_bp-lab.size_bp)<0.05)
   ||{x:(box.x0+box.x1)/2,y:G.bottom,peak:{size_bp:lab.size_bp,scan:lab.scan,height:lab.height??0},tag:lab.channel,sample:box.sample};
  view.arrowLabels=true;selectPeak(pp,multi);return}
 const z=zoneAt(G,x,y);
 if(z==='x'){drag={cv,mode:'x',x0:x,y0:y};e.preventDefault();return}
 if(z==='y'){drag={cv,mode:'y',x0:x,y0:y};e.preventDefault();return}
 if(z!=='plot')return;
 const near=nearPeak(G,x);const multi=e.ctrlKey||e.metaKey||e.shiftKey;
 // Ctrl+drag of the single selected labeled peak onto another peak moves its allele label there (ID-X Move Allele)
 if(near&&e.ctrlKey&&near.sample.editable&&view.selPeaks.size===1&&view.selPeaks.has(peakKey(near.sample,near.tag,near.peak))){
  const call=(near.sample.report?.calls||[]).find(c=>c.channel===near.tag&&Math.abs(c.size_bp-near.peak.size_bp)<0.05);
  if(call){drag={cv,mode:'move',x0:x,y0:y,call,near};e.preventDefault();return}}
 if(!near){drag={cv,mode:e.shiftKey?'sel':'h',x0:x,y0:y,moved:false,multi};e.preventDefault();return}
 // ID-X SinglePlotPane: with single-click editing a click on a labeled peak deletes its label (not on the size standard)
 if(singleClickOn()&&!multi&&near.sample.editable&&near.tag!==near.sample.analysis?.sizing?.reference_channel){const lab=labelAt(near.sample,near.tag,near.peak.size_bp);
  if(lab){e.preventDefault();deleteLabelsOf([{s:near.sample,label:lab}]);return}}
 view.arrowLabels=false;selectPeak(near,multi)});
function selectPeak(near,multi){const k=peakKey(near.sample,near.tag,near.peak);if(!multi){const had=view.selPeaks.has(k)&&view.selPeaks.size===1;view.selPeaks.clear();if(!had)view.selPeaks.set(k,near)}
 else if(view.selPeaks.has(k))view.selPeaks.delete(k);else view.selPeaks.set(k,near);repaint();showPhr()}
document.addEventListener('mousemove',e=>{if(!drag)return;const r=drag.cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
 if(Math.abs(x-drag.x0)+Math.abs(y-drag.y0)>4)drag.moved=true;if(drag.moved&&drag.mode!=='move')bandShow(drag.cv,drag,x,y)});
// Calculating Peak Height Ratio: Ctrl+click two peaks, the ratio (smaller / larger height) is shown in the status bar
function showPhr(){
 try{build()}catch(e){}
 const c=$('phr');if(!c)return;const sel=[...view.selPeaks.values()];
 if(sel.length!==2){c.textContent='';return}const [a,b]=sel.map(p=>p.peak.height);c.textContent=fmt('phrStatus',(Math.min(a,b)/Math.max(a,b)).toFixed(2),Math.round(a),Math.round(b))}
document.addEventListener('mouseup',e=>{if(!drag)return;const d=drag;drag=null;bandHide();const G=d.cv._geom,r=d.cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
 const invX=px=>G.x0+(px-G.L)/(G.W-G.L-G.R)*(G.x1-G.x0),invY=py=>(G.bottom-py)/(G.bottom-G.top)*G.ymax;
 if(d.mode==='move'){if(!d.moved)return;const t=nearPeak(G,x);if(!t||t===d.near||t.tag!==d.near.tag||t.sample!==d.near.sample)return;const bp=t.peak.size_bp;
  const mk=G.markers.find(m=>m.name===d.call.marker);if(!mk||bp<mk.min_bp||bp>mk.max_bp)return;
  const taken=(t.sample.report?.calls||[]).some(c=>c.channel===t.tag&&Math.abs(c.size_bp-bp)<0.05&&c!==d.call);if(taken)return;
  edit(t.sample,{action:'move',marker:mk.name,callId:d.call.id,new:binLabel(mk,bp),peak:{scan:t.peak.scan,size_bp:bp,height:t.peak.height,channel:t.tag}});return}
 if(d.mode==='x'){const a=invX(Math.min(d.x0,x)),b=invX(Math.max(d.x0,x));if(b-a>1&&Math.abs(x-d.x0)>3){view.x0=a;view.x1=b;view.userZoom=true;draw()}return}
 if(d.mode==='y'){if(Math.abs(y-d.y0)>3){yZoomTo(invY(Math.min(d.y0,y)))}return}
 if(d.mode==='h'){if(!d.moved||Math.abs(y-d.y0)<=4){if(!d.multi&&view.selPeaks.size){view.selPeaks.clear();repaint();showPhr()}return}yZoomTo(invY(Math.min(d.y0,y)));return}
 if(d.mode==='sel'){const lo=Math.min(d.x0,x),hi=Math.max(d.x0,x);if(!d.moved)return;
  for(const p of G.peakPts)if(p.x>=lo&&p.x<=hi)view.selPeaks.set(peakKey(p.sample,p.tag,p.peak),p);repaint();showPhr()}});
// Y zoom: all panes take the entered value as the Y maximum (ID-X: Zoom To on the Y axis; Full View returns to individual scaling)
function yZoomTo(v){if(!(v>1))return;view.yAxis='entered';view.yValue=Math.round(v);build();draw()}
function xAxisTo(a){if(view.xAxis===a)return;view.xAxis=a;view.x0=view.x1=null;view.userZoom=false;build();draw()}
function yAxisTo(a){view.yAxis=a;build();draw()}
function yFullView(){view.yAxis='individual';build();draw()}
function xFullView(){view.x0=view.x1=null;view.userZoom=false;draw()}
function xZoomToDlg(){const [d0,d1]=domain();const a=view.x0??d0,b=view.x1??d1;const v=prompt(T('zoomToXPrompt'),(+a).toFixed(1)+' - '+(+b).toFixed(1));if(v==null)return;
 const n=v.match(/-?\d+(?:[.,]\d+)?/g);if(!n||n.length<2)return;const lo=+n[0].replace(',','.'),hi=+n[1].replace(',','.');if(hi>lo){view.x0=lo;view.x1=hi;view.userZoom=true;draw()}}
function yZoomToDlg(){const v=prompt(T('zoomToYPrompt'),String(view.yValue||''));if(v==null)return;const n=parseFloat(String(v).replace(',','.'));if(n>1)yZoomTo(n)}
// Double click: X axis = full view (Y too when Y is entered), Y axis = full Y view; elsewhere nothing, as in ID-X.
document.addEventListener('dblclick',e=>{const cv=e.target.closest('canvas');if(!cv?._geom)return;const r=cv.getBoundingClientRect(),z=zoneAt(cv._geom,e.clientX-r.left,e.clientY-r.top);
 if(z==='x'){xFullView();if(view.yAxis==='entered')yFullView()}else if(z==='y')yFullView();
 else if(z==='plot'){const G=cv._geom,x=e.clientX-r.left,near=nearPeak(G,x);if(!near||!near.sample.editable)return;const bp=near.peak.size_bp;
  const labeled=(near.sample.report?.calls||[]).some(c=>c.channel===near.tag&&Math.abs(c.size_bp-bp)<0.05)||(near.sample.report?.artifacts||[]).some(a=>a.channel===near.tag&&Math.abs(a.size_bp-bp)<0.05);
  const marker=G.markers.find(m=>m.channel===near.tag&&bp>=m.min_bp&&bp<=m.max_bp);if(labeled||!marker)return;
  const peak={scan:near.peak.scan,size_bp:bp,height:near.peak.height,channel:near.tag};
  ctxMenu(e.clientX,e.clientY,[{label:T('addAlleleLabel')+` (${bp.toFixed(2)} bp)`,run:()=>edit(near.sample,{action:'add',marker:marker.name,new:binLabel(marker,bp),peak})}])}});
// Right arrow / left arrow: move the selection to the next / previous peak of the pane (Shift adds to the selection), wrapping round, as in ID-X.
document.addEventListener('keydown',e=>{if(e.key!=='ArrowLeft'&&e.key!=='ArrowRight')return;if(e.ctrlKey||e.metaKey||e.target.closest('input,textarea,select'))return;
 const last=[...view.selPeaks.keys()].pop();if(!last)return;
 // ID-X SinglePlotPane.keyPressed: after a click on a label the arrows step through the allele labels of the pane, otherwise through its peaks
 for(const cv of document.querySelectorAll('canvas')){const pts=(cv._geom?.peakPts||[]).filter(p=>!view.arrowLabels||labelAt(p.sample,p.tag,p.peak.size_bp)?.call||peakKey(p.sample,p.tag,p.peak)===last).sort((a,b)=>a.x-b.x);const i=pts.findIndex(p=>peakKey(p.sample,p.tag,p.peak)===last);if(i<0)continue;
  const dir=e.key==='ArrowRight'?1:pts.length-1;let j=(i+dir)%pts.length;if(e.altKey)for(let g=0;g<pts.length&&Math.abs(pts[j].x-pts[i].x)<0.5;g++)j=(j+dir)%pts.length; // Alt: next peak at another scan position
  const n=pts[j];e.preventDefault();if(!e.shiftKey)view.selPeaks.clear();view.selPeaks.set(peakKey(n.sample,n.tag,n.peak),n);repaint();showPhr();return}});
document.addEventListener('mousemove',e=>{const cv=e.target.closest?.('canvas');if(!cv?._geom)return;const G=cv._geom,r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
 const xv=G.x0+(x-G.L)/(G.W-G.L-G.R)*(G.x1-G.x0),v=(G.bottom-y)/(G.bottom-G.top)*G.ymax;const bp=G.dp?scanToBp(G.curve,xv):xv;
 $('xy').textContent=`[X ${G.dp?Math.round(xv):xv.toFixed(2)}  Y ${Math.round(v)}]`;
 const m=G.markers.find(m=>bp>=m.min_bp&&bp<=m.max_bp);$('markerInfo').textContent=m?`[${T('colMarker')}: ${m.name} (${m.min_bp.toFixed(2)}-${m.max_bp.toFixed(2)})]`:''});
// ---------- allele label editing (context menu) ----------
// File > Export Table: genotypes of the samples shown (ID-X: export one single-source sample as a known genotype file).
function exportPlotTable(){const ss=(payload?.samples||[]).filter(s=>s.markers?.length);if(!ss.length)return message(T('selectSamplesFirst'));
 const n=Math.max(2,...ss.flatMap(s=>s.markers.map(m=>m.calls.filter(c=>c.label).length)));
 const head=['Sample File','Sample Name','Marker',...Array.from({length:n},(_,i)=>'Allele '+(i+1))];
 const rows=ss.flatMap(s=>s.markers.map(m=>{const al=m.calls.filter(c=>c.label).map(c=>String(c.label));return [s.file,s.name,m.marker,...Array.from({length:n},(_,i)=>al[i]||'')]}));
 const text=[head,...rows].map(r=>r.join('\t')).join('\r\n')+'\r\n';const name=(ss.length===1?ss[0].name:'Genotypes')+'.txt';
 const url=URL.createObjectURL(new Blob([text],{type:'text/plain'}));const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)}
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
// Context menu with the menu bar's look: shortcuts, check marks, submenus (Rename Allele Label >).
function ctxMenu(x,y,items){document.querySelectorAll('.menu.ctx').forEach(m=>m.remove());
 const before=new Set(document.querySelectorAll('body>.menu'));const m=buildMenu(items);document.body.append(m);
 for(const n of document.querySelectorAll('body>.menu'))if(!before.has(n))n.classList.add('ctx');
 m.classList.add('show');m.style.left=x+'px';m.style.top=y+'px';
 const r=m.getBoundingClientRect();if(r.right>innerWidth)m.style.left=Math.max(0,x-r.width)+'px';if(r.bottom>innerHeight)m.style.top=Math.max(0,y-r.height)+'px';
 setTimeout(()=>document.addEventListener('mousedown',function h(e){if(!e.target.closest('.menu.ctx')){document.querySelectorAll('.menu.ctx').forEach(n=>n.remove());document.removeEventListener('mousedown',h)}}),0)}
function ask(title,fields){return new Promise(res=>{dialog(title,b=>{for(const f of fields)b.append(el('div',{class:'field'},el('label',{text:f.label}),el('input',{type:'text',id:f.id,value:f.value||''})))},
 [[T('ok'),b=>{const out={};for(const f of fields){out[f.id]=b.querySelector('#'+f.id).value.trim();if(!out[f.id]){message(T('reasonRequired'));return false}}
   res(out);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
// Predefined artifact labels (Add or Rename Custom Artifact Label dialog), shared by all windows of this browser.
const ART_KEY='opentrace.artifactLabels';
const artList=()=>{try{const v=JSON.parse(localStorage.getItem(ART_KEY)||'[]');return Array.isArray(v)?v.filter(x=>typeof x==='string'):[]}catch(e){return []}};
const artSave=l=>{try{localStorage.setItem(ART_KEY,JSON.stringify(l))}catch(e){}};
function artifactDialog(title,current){return new Promise(res=>{const list=artList();
 dialog(title,b=>{b.classList.add('artdlg');const sel=el('select',{id:'arSel',size:'6'});for(const l of list)sel.append(new Option(l,l));sel.append(new Option(T('customArtifact'),'__custom__'));
  sel.value=list.includes(current)?current:'__custom__';const txt=el('input',{type:'text',id:'arText',value:list.includes(current)?'':current||'',maxlength:'40'});
  const add=el('label',{class:'chk'},el('input',{type:'checkbox',id:'arAdd'}),document.createTextNode(' '+T('addToPredefined')));
  const sync=()=>{const c=sel.value==='__custom__';txt.disabled=!c;add.querySelector('input').disabled=!c};sel.addEventListener('change',sync);sync();
  b.append(el('div',{class:'field'},el('label',{text:T('predefinedLabels')}),sel),el('div',{class:'field'},el('label',{text:T('customArtifact')}),txt),add)},
  [[T('ok'),b=>{const c=b.querySelector('#arSel').value==='__custom__';const v=c?b.querySelector('#arText').value.trim():b.querySelector('#arSel').value;
    if(!v||v.length>40){message(T('badBinName'));return false}
    if(c&&b.querySelector('#arAdd').checked&&!list.includes(v))artSave([...list,v]);res(v);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
function deletePredefined(){let list=artList();
 const d=dialog(T('deletePredefined'),b=>{},[[T('ok'),()=>{artSave(list);return true},true],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();const sel=el('select',{id:'dpSel',size:'8',style:'min-width:240px'});for(const l of list)sel.append(new Option(l,l));
  const del=el('button',{class:'btn',type:'button',text:T('mgrDelete'),id:'dpDel'});del.addEventListener('click',()=>{if(sel.value){list=list.filter(x=>x!==sel.value);draw()}});
  b.append(list.length?sel:el('p',{class:'note',text:T('noPredefined')}),el('div',{class:'btnrow'},del))};draw()}
async function edit(sample,spec){
 if(window.Sec&&!Sec.can('UPDATE_PROJECT'))return Sec.denied();
 if(spec.action==='rename'||spec.action==='add'||spec.action==='artifact_to_allele'){const v=await ask(T('act_'+spec.action),[{id:'edLabel',label:T('colAllele'),value:spec.new}]);if(!v)return;
  if(v.edLabel.length>40)return message(T('badBinName'));spec={...spec,new:v.edLabel}}
 if(['to_artifact','add_artifact','rename_artifact'].includes(spec.action)){const l=await artifactDialog(T('act_'+spec.action),spec.new);if(l==null)return;spec={...spec,new:l}}
 let items=[];try{items=window.opener.OT.editItems(sample.id,spec)}catch(e){}
 const ch=window.Sec?await Sec.change({items,title:T('reasonTitle')}):{reason:''};if(!ch)return;
 spec={...spec,reason:ch.reason};
 let ok=false;try{ok=await window.opener.OT.edit(sample.id,spec)}catch(e){message(String(e.message||e))}
 if(ok)afterEdit()}
function afterEdit(){try{payload=window.opener.OT.plotPayload()}catch(e){}build();draw()}
function binLabel(marker,bp){const b=(marker.bins||[]).find(b=>bp>=b.center_bp-(b.left_bp??.5)&&bp<=b.center_bp+(b.right_bp??.5));return b?b.label:'OL'}
document.addEventListener('contextmenu',e=>{const cv=e.target.closest('canvas');const head=e.target.closest('.phead');
 if(head&&head._sample){e.preventDefault();const s=head._sample;if(!s.editable)return;
  ctxMenu(e.clientX,e.clientY,[{label:T('overrideCgq'),run:()=>edit(s,{action:'override_cgq'})}]);return}
 if(!cv?._geom)return;e.preventDefault();const G=cv._geom,r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
 {const z=zoneAt(G,x,y);
  if(z==='x'){const xv=G.x0+(x-G.L)/(G.W-G.L-G.R)*(G.x1-G.x0),bpx=G.dp?scanToBp(G.curve,xv):xv;const pm=G.markers.find(m=>bpx>=m.min_bp&&bpx<=m.max_bp)||G.markers.find(m=>m.name===view.selMarker);
   const pk=pm?G.sample.id+'|'+pm.name:null;
   ctxMenu(e.clientX,e.clientY,[{label:T('fullView'),run:xFullView},{label:T('zoomTo'),run:xZoomToDlg},
    {label:T('partialLocus')+(pm?` (${pm.name})`:''),checked:!!(pk&&view.partial?.has(pk)),disabled:!pm,run:()=>{view.partial=view.partial||new Set();view.partial.has(pk)?view.partial.delete(pk):view.partial.add(pk);draw()}}]);return}
  if(z==='y'){ctxMenu(e.clientX,e.clientY,[{label:T('fullView'),run:yFullView},{label:T('zoomTo'),run:yZoomToDlg}]);return}}
 const s=G.sample;
 const mk=(G.markerHits||[]).find(h=>x>=h.x0&&x<=h.x1&&y>=h.y0&&y<=h.y1)?.m;
 if(mk){if(!s.editable)return;const row=(s.markers||[]).find(r=>r.marker===mk.name);
  ctxMenu(e.clientX,e.clientY,[{label:T('overrideGq')+' ('+mk.name+')',run:()=>edit(s,{action:'override_gq',marker:mk.name})},
   {label:(row?.pli?'✓ ':'')+T('pliMenu'),run:()=>edit(s,{action:'pli',marker:mk.name,new:row?.pli?'off':'on'})}]);return}
 // allele / artifact label or peak: the ID-X peak popup; right-click selects the peak unless it is already selected
 const box=G.labelBoxes.find(b=>x>=b.x0&&x<=b.x1&&y>=b.y0&&y<=b.y1);let near=null;
 if(box){const lab=box.call||box.artifact;near=G.peakPts.find(p=>p.sample===box.sample&&p.tag===lab.channel&&Math.abs(p.peak.size_bp-lab.size_bp)<0.05)
  ||{x:(box.x0+box.x1)/2,y:G.bottom,peak:{size_bp:lab.size_bp,scan:lab.scan,height:lab.height??0},tag:lab.channel,sample:box.sample}}
 else near=nearPeak(G,x);
 if(near){const k=peakKey(near.sample,near.tag,near.peak);if(!view.selPeaks.has(k)){view.selPeaks.clear();view.selPeaks.set(k,near);repaint();showPhr()}
  ctxMenu(e.clientX,e.clientY,peakMenu(G,near));return}
 if(!s.editable)return;
 // blank area of a pane: Delete All Labels for Sample
 ctxMenu(e.clientX,e.clientY,[{label:T('deleteAllForSample'),run:()=>deleteLabels([s],null)}])});
// Delete All Labels for Sample / Alleles > Delete Multiple Labels (All Labels, Labels In Range): one confirmation and one reason
// for all allele and artifact labels of the given samples (all displayed samples for the Alleles menu).
async function deleteLabels(samples,range){if(window.Sec&&!Sec.can('UPDATE_PROJECT'))return Sec.denied();
 const inR=v=>!range||(v>=range[0]&&v<=range[1]);const specs=[];
 for(const s of samples.filter(x=>x.editable)){
  for(const c of s.report?.calls||[])if(inR(c.size_bp))specs.push({sample:s,spec:{action:'delete',marker:c.marker,callId:c.id}});
  for(const a of s.report?.artifacts||[])if(inR(a.size_bp))specs.push({sample:s,spec:{action:'delete_artifact',artifactId:a.id,marker:a.marker}})}
 if(!specs.length)return message(T('noLabelsToDelete'));
 if(!(await new Promise(res=>dialog(T('alert'),b=>b.append(el('p',{text:fmt('confirmDeleteLabels',specs.length,samples.length)})),[[T('yes'),()=>{res(true);return true},true],[T('no'),()=>{res(false);return true}]]))))return;
 const n=await runBatch(specs.map(x=>({s:x.sample,spec:x.spec})));if(n)status(fmt('deletedLabels',n))}
function deleteInRangeDialog(){const [d0,d1]=bpRange();
 dialog(T('deleteInRangeTitle'),b=>{b.classList.add('dirdlg');b.append(el('p',{text:T('deleteInRangeText')}),
  el('div',{class:'field'},el('label',{text:T('psStartRange')}),el('input',{type:'text',id:'dirFrom',class:'num',value:String(Math.round(view.x0??d0))})),
  el('div',{class:'field'},el('label',{text:T('psEndRange')}),el('input',{type:'text',id:'dirTo',class:'num',value:String(Math.round(view.x1??d1))})))},
  [[T('ok'),b=>{const a=+b.querySelector('#dirFrom').value,z=+b.querySelector('#dirTo').value;if(!(isFinite(a)&&isFinite(z)&&z>a)){message(T('psEndGtStart'));return false}
    setTimeout(()=>deleteLabels(payload.samples,[a,z]),0);return true},true],[T('cancel'),()=>true]])}
// ---------- label editing on the selected peaks (ID-X Alleles menu and peak popup; Delete, Ctrl+L, Ctrl+I) ----------
const SC_KEY='opentrace.plot.singleClickEdit';
view.singleClick=(()=>{try{return localStorage.getItem(SC_KEY)==='1'}catch(e){return false}})();
// ID-X (PlotMgrPane.onAlleleMenuSelection) disables single-click editing when labels are off and in overlay plots
function singleClickAllowed(){return view.labels!=='none'&&view.layout!=='overlay'}
function singleClickOn(){return !!view.singleClick&&singleClickAllowed()}
function setSingleClick(on){view.singleClick=!!on;try{localStorage.setItem(SC_KEY,on?'1':'0')}catch(e){}build()}
function sampleById(id){return (payload?.samples||[]).find(s=>s.id===id)}
function labelAt(s,tag,bp){const c=(s.report?.calls||[]).find(c=>c.channel===tag&&Math.abs(c.size_bp-bp)<0.05);if(c)return {call:c};
 const a=(s.report?.artifacts||[]).find(a=>a.channel===tag&&Math.abs(a.size_bp-bp)<0.05);return a?{artifact:a}:null}
function markerOf(s,tag,bp){return ((s.kit||payload?.kit)?.markers||[]).find(m=>m.channel===tag&&bp>=m.min_bp&&bp<=m.max_bp)}
function isSizeStd(s,tag){return tag===s.analysis?.sizing?.reference_channel}
// selected peaks resolved against the current payload (labels change after every edit); size-standard peaks are not edited here
function selection(){const out=[];for(const p of view.selPeaks.values()){const s=sampleById(p.sample.id);if(!s||isSizeStd(s,p.tag))continue;const bp=p.peak.size_bp;
  out.push({s,tag:p.tag,peak:p.peak,bp,label:labelAt(s,p.tag,bp),marker:markerOf(s,p.tag,bp)})}
 return out.sort((a,b)=>a.s.id-b.s.id||a.bp-b.bp)}
function canEdit(){return !(window.Sec&&!Sec.can('UPDATE_PROJECT'))}
function peakSpec(p){return {scan:p.peak.scan,size_bp:p.bp,height:p.peak.height,channel:p.tag}}
function delSpec(l){return l.call?{action:'delete',marker:l.call.marker,callId:l.call.id}:{action:'delete_artifact',artifactId:l.artifact.id,marker:l.artifact.marker}}
function status(t){$('markerInfo').textContent=t}
// Several edits with one reason, applied together (the project table is redrawn once).
async function runBatch(list){if(!canEdit()){Sec.denied();return 0}list=list.filter(x=>x.s.editable);if(!list.length)return 0;
 let items=[];try{for(const x of list)items.push(...window.opener.OT.editItems(x.s.id,x.spec))}catch(e){}
 const ch=window.Sec?await Sec.change({items,title:T('reasonTitle')}):{reason:''};if(!ch)return 0;
 let n=0;try{n=await window.opener.OT.editMany(list.map(x=>({id:x.s.id,spec:x.spec})),ch.reason)}catch(e){message(String(e.message||e))}
 afterEdit();return n}
async function deleteLabelsOf(list){const n=await runBatch(list.filter(x=>x.label).map(x=>({s:x.s,spec:delSpec(x.label)})));if(n)status(fmt('deletedLabels',n))}
// Delete Selected Label(s) (Delete): the labels of all selected peaks of all panes
function deleteSelected(){const sel=selection().filter(p=>p.label);if(!sel.length)return message(T('noLabelsSelected'));return deleteLabelsOf(sel)}
// Add Allele Label (Ctrl+L): one peak asks for the label (bin name proposed), several get their bin names
async function addAlleleSelected(){const sel=selection().filter(p=>!p.label&&p.marker);if(!sel.length)return message(T('noPeaksSelected'));
 if(sel.length===1){const p=sel[0];return edit(p.s,{action:'add',marker:p.marker.name,new:binLabel(p.marker,p.bp),peak:peakSpec(p)})}
 const n=await runBatch(sel.map(p=>({s:p.s,spec:{action:'add',marker:p.marker.name,new:binLabel(p.marker,p.bp),peak:peakSpec(p)}})));if(n)status(fmt('addedLabels',n))}
// Add Artifact Label (Ctrl+I): unlabeled peaks get the artifact label, allele labels are changed to it (one label, one reason)
async function addArtifactSelected(preset){const sel=selection().filter(p=>!p.label?.artifact);if(!sel.length)return message(T('noPeaksSelected'));
 const label=preset??await artifactDialog(T('act_add_artifact'),'');if(label==null)return;
 const n=await runBatch(sel.map(p=>({s:p.s,spec:p.label?.call?{action:'to_artifact',marker:p.label.call.marker,callId:p.label.call.id,new:label}:{action:'add_artifact',marker:p.marker?.name||null,new:label,peak:peakSpec(p)}})));
 if(n)status(fmt('changedLabels',n))}
// Rename Allele Label > Custom allele label / Custom artifact label / a predefined artifact label (ID-X PlotMgrPane.updateRenameAlleleList)
async function renameAlleles(choice){const sel=selection().filter(p=>p.label?.call);if(!sel.length)return message(T('noLabelsSelected'));
 if(choice==='allele'){if(sel.length===1){const c=sel[0].label.call;return edit(sel[0].s,{action:'rename',marker:c.marker,callId:c.id,new:c.label})}
  const v=await ask(T('renameAllele'),[{id:'edLabel',label:T('colAllele'),value:String(sel[0].label.call.label)}]);if(!v)return;if(v.edLabel.length>40)return message(T('badBinName'));
  const n=await runBatch(sel.map(p=>({s:p.s,spec:{action:'rename',marker:p.label.call.marker,callId:p.label.call.id,new:v.edLabel}})));if(n)status(fmt('changedLabels',n));return}
 const label=choice==='artifact'?await artifactDialog(T('act_to_artifact'),''):choice;if(label==null)return;
 const n=await runBatch(sel.map(p=>({s:p.s,spec:{action:'to_artifact',marker:p.label.call.marker,callId:p.label.call.id,new:label}})));if(n)status(fmt('changedLabels',n))}
async function renameArtifacts(choice){const sel=selection().filter(p=>p.label?.artifact);if(!sel.length)return message(T('noLabelsSelected'));
 const label=choice==='artifact'?await artifactDialog(T('act_rename_artifact'),sel[0].label.artifact.label):choice;if(label==null)return;
 const n=await runBatch(sel.map(p=>({s:p.s,spec:{action:'rename_artifact',artifactId:p.label.artifact.id,marker:p.label.artifact.marker,new:label}})));if(n)status(fmt('changedLabels',n))}
function renameAlleleItems(){return [{label:T('customAlleleLabel'),run:()=>renameAlleles('allele')},{label:T('customArtifactLabel'),run:()=>renameAlleles('artifact')},...artList().map(l=>({label:l,run:()=>renameAlleles(l)}))]}
function renameArtifactItems(){return [{label:T('customArtifactLabel'),run:()=>renameArtifacts('artifact')},...artList().map(l=>({label:l,run:()=>renameArtifacts(l)}))]}
function renameOne(){const p=selection().find(p=>p.label);if(!p)return message(T('selectOneLabel'));
 if(p.label.call)return edit(p.s,{action:'rename',marker:p.label.call.marker,callId:p.label.call.id,new:p.label.call.label});
 return edit(p.s,{action:'rename_artifact',artifactId:p.label.artifact.id,marker:p.label.artifact.marker,new:p.label.artifact.label})}
function openHistory(){try{window.opener.OT.labelEditViewer();window.opener.focus()}catch(e){}}
// Peak popup as in ID-X (SinglePlotPane): allele label -> Delete Label(s), Rename Allele Label >, History (edited labels), Peak Raw Data;
// artifact label -> Rename Artifact Label >, Delete Label(s), Add Allele Label, History, Peak Raw Data;
// no label -> Add Artifact Label, Delete Label(s), Add Allele Label, Peak Raw Data. Editing needs rights, an editable sample and a basepair X axis.
function peakMenu(G,near){const s=sampleById(near.sample.id)||near.sample,bp=near.peak.size_bp,lab=labelAt(s,near.tag,bp);
 const ed=!!s.editable&&canEdit()&&!isSizeStd(s,near.tag)&&view.xAxis==='bp';const marker=markerOf(s,near.tag,bp);
 const raw={label:T('peakRawData'),run:()=>{try{window.opener.OT.peakRawData(s.id,near.peak.scan);window.opener.focus()}catch(e){message(String(e.message||e))}}};
 const hist={label:T('history'),run:openHistory};
 const del={label:T('deleteLabelsPopup'),sc:'Del',disabled:!ed||!lab,run:deleteSelected};
 const addAl={label:T('addAlleleLabel'),sc:'Ctrl+L',disabled:!ed||!marker,run:()=>lab?.artifact?edit(s,{action:'artifact_to_allele',artifactId:lab.artifact.id,marker:marker.name,new:lab.artifact.orig??binLabel(marker,bp)}):addAlleleSelected()};
 if(lab?.call)return [del,{label:T('renameAllele'),disabled:!ed,sub:ed?renameAlleleItems():undefined},'-',...(lab.call.user_edited?[hist]:[]),raw];
 if(lab?.artifact)return [{label:T('renameArtifactMenu'),disabled:!ed,sub:ed?renameArtifactItems():undefined},del,addAl,'-',hist,raw];
 return [{label:T('addArtifactLabel'),sc:'Ctrl+I',disabled:!ed,run:()=>addArtifactSelected()},del,addAl,'-',raw]}
// "Edit labels" bar: the same commands as buttons, enabled by the selection, with a hint line.
const EB_ICONS={
 clean:'<svg viewBox="0 0 16 16"><path d="M9.5 1.5l5 5-6 6H4l-2.5-2.5z" fill="#f4c7c3" stroke="#a1231b"/><path d="M5.5 6.5l5 5" stroke="#a1231b"/><path d="M1 15h14" stroke="#555"/></svg>',
 addAllele:'<svg viewBox="0 0 16 16"><rect x="1.5" y="8.5" width="9" height="6" fill="#fff" stroke="#1f4fd6"/><path d="M4 11.5h4" stroke="#1f4fd6"/><path d="M12.5 1v6M9.5 4h6" stroke="#2ea043" stroke-width="2"/></svg>',
 addArtifact:'<svg viewBox="0 0 16 16"><rect x="1.5" y="8.5" width="9" height="6" fill="#fff" stroke="#e040a0" stroke-width="1.5"/><path d="M4 11.5h4" stroke="#b0006a"/><path d="M12.5 1v6M9.5 4h6" stroke="#2ea043" stroke-width="2"/></svg>',
 rename:'<svg viewBox="0 0 16 16"><rect x="1.5" y="8.5" width="9" height="6" fill="#fff" stroke="#555"/><path d="M8 7l1-3 5-3 1 1.5-4.5 4z" fill="#f2c94c" stroke="#8a6d1c"/></svg>',
 delete:'<svg viewBox="0 0 16 16"><rect x="1.5" y="8.5" width="9" height="6" fill="#fff" stroke="#555"/><path d="M10 1.5l5 5M15 1.5l-5 5" stroke="#d93025" stroke-width="2"/></svg>',
 range:'<svg viewBox="0 0 16 16"><path d="M1 13h14" stroke="#555"/><path d="M3 13V6M6 13V3M10 13V5M13 13V8" stroke="#1f4fd6"/><rect x="4.5" y="1.5" width="7" height="12" fill="rgba(217,48,37,.15)" stroke="#d93025" stroke-dasharray="2 1"/></svg>',
 sample:'<svg viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" fill="#fff" stroke="#555"/><path d="M2 12l2-6 1 6 2-8 1 8 2-4 1 4" stroke="#1f4fd6" fill="none"/><path d="M9.5 1.5l5 5M14.5 1.5l-5 5" stroke="#d93025" stroke-width="1.8"/></svg>',
 undo:'<svg viewBox="0 0 16 16"><path d="M4 6h6a4 4 0 0 1 0 8H6" fill="none" stroke="#2f6fd6" stroke-width="1.8"/><path d="M1 6l4-4v8z" fill="#2f6fd6"/></svg>',
 history:'<svg viewBox="0 0 16 16"><rect x="1.5" y="2.5" width="13" height="11" fill="#fff" stroke="#555"/><path d="M3 5h10M3 8h10M3 11h6" stroke="#777"/></svg>'};
function renderEditBar(){const bar=$('editbar');if(!bar)return;
 const any=(payload?.samples||[]).some(s=>s.editable)&&canEdit();const sel=selection();const nl=sel.filter(p=>p.label).length,nf=sel.filter(p=>!p.label&&p.marker).length,na=sel.filter(p=>!p.label?.artifact).length;
 const cur=sel[0]?.s||sampleById(view.selSample)||(payload?.samples?.length===1?payload.samples[0]:null);
 const b=(id,text,tip,run,dis,on)=>{const x=el('button',{class:'eb'+(on?' on':''),type:'button',title:tip,id:'eb_'+id});x.innerHTML=EB_ICONS[id];x.append(el('span',{text}));x.disabled=!!dis;x.addEventListener('click',()=>run());return x};
 const sep=()=>el('span',{class:'ebsep'});
 bar.replaceChildren(el('span',{class:'ebtitle',text:T('ebTitle')}),
  b('clean',T('ebClean'),T('ebTipClean'),()=>setSingleClick(!view.singleClick),!any||!singleClickAllowed(),singleClickOn()),sep(),
  b('addAllele',T('ebAddAllele'),T('ebTipAddAllele'),addAlleleSelected,!any||!nf),
  b('addArtifact',T('ebAddArtifact'),T('ebTipAddArtifact'),()=>addArtifactSelected(),!any||!na),
  b('rename',T('ebRename'),T('ebTipRename'),renameOne,!any||nl!==1),
  b('delete',T('ebDelete'),T('ebTipDelete'),deleteSelected,!any||!nl),sep(),
  b('range',T('ebRange'),T('ebTipRange'),deleteInRangeDialog,!any),
  b('sample',T('ebSample')+(cur&&payload.samples.length>1?` (${cur.name})`:''),T('ebTipSample'),()=>deleteLabels([cur],null),!any||!cur),sep(),
  b('undo',T('ebUndo'),T('ebTipUndo'),undoLast,!any),b('history',T('ebHistory'),T('ebTipHistory'),openHistory,false),
  el('span',{class:'ebhint',id:'ebHint',text:!any?T('ebReadOnly'):singleClickOn()?T('ebHintClean'):sel.length?fmt('ebHintSel',sel.length,nl):T('ebHintIdle')}));
 document.body.classList.toggle('cleanmode',singleClickOn()&&any)}
// Multiple Dye Plot: all dyes of a sample within the range of the selected (or first) marker
// ID-X GenotypeMultipleDyePane: "Sample File" and "Marker" lines, View > Combine Dyes (default) / Separate Dyes, the dyes
// switched on in the plot window, the X range of the pane (its zoom, or the marker range with the Marker Margin), and the
// mouse position in the status line.
function multipleDyePlot(){
 const cv=[...document.querySelectorAll('#plotarea canvas')].find(c=>c._geom&&(view.selSample==null||c._geom.sample.id===view.selSample))||document.querySelector('#plotarea canvas');
 const G=cv?._geom;if(!G)return message(T('selectSamplesFirst'));const s=G.sample;
 const m=G.markers.find(x=>x.name===view.selMarker)||(isGeno()?G.markers[0]:null);if(!m)return message(T('selectMarkerHeader'));
 const tags=(s.analysis?.channels||[]).filter(c=>c.group!=='auxiliary'&&DYE_KEYS.includes(c.tag)&&view.dyes.has(c.tag)).map(c=>c.tag);
 const mg=setting?.display?.markerMargin??5,cur=curveOf(s.analysis);let a=m.min_bp-mg,z=m.max_bp+mg;if(view.xAxis==='dp'){a=bpToScan(cur,a);z=bpToScan(cur,z)}
 if(view.userZoom&&view.x0!=null&&view.x1!=null){a=view.x0;z=view.x1}
 let mode='combine';
 dialog(T('mdpTitle'),b=>{b.classList.add('mdpdlg');
  const host=el('div',{class:'mdphost'}),st=el('div',{class:'mdpstatus',text:' '});
  const radio=(v,label)=>{const r=el('input',{type:'radio',name:'mdpMode',value:v,id:'mdp_'+v});r.checked=mode===v;r.addEventListener('change',()=>{mode=v;render()});return el('label',{class:'chk'},r,document.createTextNode(' '+label))};
  const render=()=>{host.replaceChildren();const groups=mode==='combine'?[tags]:tags.map(t=>[t]);const h=mode==='combine'?320:Math.max(180,Math.floor(640/Math.max(1,groups.length)));
   for(const g of groups){const c=el('canvas',{style:'width:900px;max-width:86vw;display:block'});c.height=h;host.append(c);requestAnimationFrame(()=>paint(c,[{sample:s,dyes:g}],a,z,m))}};
  host.addEventListener('mousemove',e=>{const c=e.target.closest('canvas');const g=c?._geom;if(!g)return;const r=c.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
   const xv=g.x0+(x-g.L)/(g.W-g.L-g.R)*(g.x1-g.x0),v=(g.bottom-y)/(g.bottom-g.top)*g.ymax;st.textContent=`[X ${g.dp?Math.round(xv):xv.toFixed(2)}  Y ${Math.round(v)}]`});
  b.append(el('div',{class:'mdphead'},el('div',{text:fmt('mdpSampleFile',s.file)}),el('div',{text:fmt('mdpMarker',m.name)})),
   el('div',{class:'mdpmodes'},radio('combine',T('combine')),radio('separate',T('separate'))),host,st);
  render()},[[T('closeWin'),()=>true,true]])}
// ---------- Edit / View menu ----------
function curSample(){return payload?.samples.find(s=>s.id===view.selSample)||payload?.samples[0]||null}
function sampleView(tab){const s=curSample();if(!s)return;try{window.opener.OT.showSampleView(s.id,tab);window.opener.focus()}catch(e){message(String(e.message||e))}}
async function undoLast(){if(!payload)return;if(window.Sec&&!Sec.can('UPDATE_PROJECT'))return Sec.denied();
 let r=null;try{r=window.opener.OT.undoLast(payload.samples.map(s=>s.id))}catch(e){return message(String(e.message||e))}
 if(!r)return message(T('nothingToUndo'));try{payload=window.opener.OT.plotPayload()}catch(e){}build();draw();
 $('markerInfo').textContent=fmt('undoneEdit',T('act_'+r.action)===('act_'+r.action)?r.action:T('act_'+r.action),r.marker,r.sample)}
function copyPlotTable(){
 // the sizing table is virtual: copy all of its rows from the data, not from the DOM
 const rows=view.table==='sizing'&&sizing?[[...sizing.table.tHead.rows[0].cells].map(c=>c.textContent).join('\t'),...sizing.rows.map(r=>[r.s.name,...sizing.scols.map(([id])=>({sampleFileName:r.s.file,marker:r.call?.marker||'',
   dyeSamplePeak:`${DYE_LETTER[r.tag]||'?'},${r.n}${r.star?' *':''}`,minutes:'',size:r.pk.size_bp.toFixed(2),height:String(Math.round(r.pk.height)),area:String(r.pk.area??''),dataPoint:String(Math.round(r.pk.scan)),alleleCall:r.call?String(r.call.label):''})[id])].join('\t'))].join('\n')
  :[...document.querySelectorAll('#gtGrid table tr')].map(tr=>[...tr.children].map(c=>c.textContent).join('\t')).join('\n');
 if(!rows)return;try{navigator.clipboard.writeText(rows);$('markerInfo').textContent=T('copiedTable')}catch(e){message(String(e.message||e))}}

// ---------- plot printing (Page Setup > Plot tab: Honor Plots per Pane, Small, Medium, Large) ----------
const PAGE_KEY='opentrace.pageSetup.v1';
const SIZES={A4:[210,297],Letter:[216,279],Legal:[216,356]};
function pageOpts(){let v={};try{v=JSON.parse(localStorage.getItem(PAGE_KEY)||'{}')||{}}catch(e){}
 return {...v,orientation:v.orientation==='portrait'?'portrait':'landscape',paper:['A4','Letter','Legal'].includes(v.paper)?v.paper:'A4',
  header:v.header!==false,plot:['honor','small','medium','large'].includes(v.plot)?v.plot:'honor',headerStyle:v.headerStyle==='outline'?'outline':'filled'}}
function plotPageSetup(){const o=pageOpts();
 dialog(T('pageSetup').replace(/\.+$/,''),b=>{b.classList.add('ppsdlg');
  const paper=el('select',{id:'ppPaper'});for(const p of Object.keys(SIZES))paper.append(new Option(p,p));paper.value=o.paper;paper.addEventListener('change',()=>o.paper=paper.value);
  const or=el('select',{id:'ppOrient'});or.append(new Option(T('psLandscape'),'landscape'),new Option(T('psPortrait'),'portrait'));or.value=o.orientation;or.addEventListener('change',()=>o.orientation=or.value);
  const fs=el('fieldset',{},el('legend',{text:T('ppPlot')}));
  for(const [v,k] of [['honor','ppHonor'],['small','ppSmall'],['medium','ppMedium'],['large','ppLarge']]){const r=el('input',{type:'radio',name:'ppPlot',value:v,'data-pp':v});r.checked=o.plot===v;
   r.addEventListener('change',()=>{if(r.checked)o.plot=v});fs.append(el('label',{class:'chk'},r,document.createTextNode(' '+T(k))))}
  fs.append(el('div',{class:'note',text:T('ppHonorNote')}));
  const hd=el('input',{type:'checkbox',id:'ppHeader'});hd.checked=o.header;hd.addEventListener('change',()=>o.header=hd.checked);
  const mh=el('select',{id:'ppMarkerHeader'});mh.append(new Option(T('ppHeadFilled'),'filled'),new Option(T('ppHeadOutline'),'outline'));mh.value=o.headerStyle;mh.addEventListener('change',()=>o.headerStyle=mh.value);
  b.append(el('div',{class:'field'},el('label',{text:T('psPaper')}),paper),el('div',{class:'field'},el('label',{text:T('psOrientation')}),or),fs,
   el('label',{class:'chk'},hd,document.createTextNode(' '+T('psHeaderText'))),el('div',{class:'field'},el('label',{text:T('ppMarkerHeaders')}),mh))},
  [[T('ok'),()=>{try{localStorage.setItem(PAGE_KEY,JSON.stringify(o))}catch(e){}return true},true],[T('cancel'),()=>true]])}
// panes per page in "Honor Plots per Pane" (help: Letter/A4 7 portrait, 5 landscape; Legal 8 portrait, 5 landscape)
function panesPerPage(o){const max=o.orientation==='portrait'?(o.paper==='Legal'?8:7):5;return Math.max(1,Math.min(view.panes,max))}
// Printed plot pages as ID-X prints them: application/version and "Project: <name>" at the top, one row of column
// headings (the sample header columns of the plot setting), every pane boxed with its own header row (sample name and
// PQV symbols in those columns), and a footer with the print date, "Printed by: <user>" and "Page i of n".
function printColumns(){const H=setting?.sampleHeader||{};const on=k=>H[k]!==false;
 const cols=[];if(on('sampleFileName'))cols.push(['file',T('colSampleFile')]);if(on('sampleName'))cols.push(['name',T('colSampleName')]);if(on('panel'))cols.push(['panel',T('colPanel')]);
 for(const k of PQV_KEYS.filter(on))cols.push(['pqv:'+k,k]);return cols}
function printPages(o){
 const [w,h]=SIZES[o.paper];const [pw,ph]=o.orientation==='portrait'?[w,h]:[h,w];const PX=3.78;   // px per mm at 96 dpi
 const usableW=pw-20,headH=o.header?20:0,colH=6,footH=9,rowH=5.5;
 const usableH=ph-20-headH-colH-footH;
 const per=o.plot==='honor'?panesPerPage(o):Math.max(1,Math.floor(usableH/({small:40,medium:60,large:90}[o.plot])));
 const paneH=usableH/per;const imgH=paneH-rowH-3;const panes=paneList();const [d0,d1]=domain();
 const host=el('div',{style:'position:absolute;left:-20000px;top:0'});document.body.append(host);const imgs=[];
 view.printing=true;view.headerStyle=o.headerStyle;
 for(const p of panes){const items=p.overlay?p.items:[p];const s=items[0].sample;
  let x0=view.x0??d0,x1=view.x1??d1;if(p.marker&&!view.userZoom){const mg=setting?.display?.markerMargin??5,c=curveOf(s.analysis);x0=p.marker.min_bp-mg;x1=p.marker.max_bp+mg;if(view.xAxis==='dp'){x0=bpToScan(c,x0);x1=bpToScan(c,x1)}}
  const cv=el('canvas',{style:`width:${Math.round((usableW-2)*PX)}px`});cv.height=Math.round(imgH*PX);host.append(cv);
  paint(cv,items,x0,x1,p.marker);
  imgs.push({src:cv.toDataURL('image/png'),sample:p.overlay?null:s,marker:p.marker||null,overlay:!!p.overlay})}
 view.printing=false;host.remove();
 const pages=[];for(let i=0;i<imgs.length;i+=per)pages.push(imgs.slice(i,i+per));
 return {pages,pw,ph,usableW,paneH,imgH,headH,colH,footH,rowH}}
function printDate(){const d=new Date(),loc=I18N.lang==='ru'?'ru-RU':'en-US';
 const wd=d.toLocaleDateString(loc,{weekday:'short'}),mo=d.toLocaleDateString(loc,{month:'short'}).replace('.','');
 const tm=d.toLocaleTimeString('en-US',{hour:'numeric',minute:'2-digit'}).replace(' ','');
 const tz=(d.toLocaleTimeString('en-US',{timeZoneName:'short'}).split(' ').pop())||'';
 return `${wd[0].toUpperCase()+wd.slice(1).replace('.','')} ${mo} ${d.getDate()},${d.getFullYear()} ${tm}, ${tz}`}
function printUser(){try{return window.opener?.Sec?.user?.name||window.opener?.Sec?.user?.user||'OpenTrace'}catch(e){return 'OpenTrace'}}
function printPlots(preview){if(!payload||!payload.samples.length)return message(T('selectSamplesFirst'));
 const o=pageOpts();const r=printPages(o);const cols=printColumns();
 let area=document.getElementById('plotPrint');if(area)area.remove();area=el('div',{id:'plotPrint',class:preview?'ppreview':''});
 const st=el('style',{id:'plotPrintStyle'});st.textContent=`@page{size:${o.paper.toLowerCase()} ${o.orientation};margin:10mm}`;document.getElementById('plotPrintStyle')?.remove();document.head.append(st);
 if(preview){const bar=el('div',{class:'ppbar noprint'});const bt=(txt,fn,id)=>{const b=el('button',{class:'btn',type:'button',text:txt,id});b.addEventListener('click',fn);return b};
  bar.append(bt(T('print'),()=>{window.print()},'ppPrint'),bt(T('closeWin'),()=>area.remove(),'ppClose'),el('span',{text:' '+fmt('ppPages',r.pages.length,o.paper,T(o.orientation==='portrait'?'psPortrait':'psLandscape'))}));area.append(bar)}
 const proj=payload.project||T('untitled');const date=printDate(),user=printUser();
 const ver=(payload.samples.map(s=>s.analysis?.analysis_version).find(Boolean)||'OpenTrace').replace('/',' ').replace(/\.0$/,'');
 // column widths: the name column takes a fifth of the width, PQV columns share the rest (as in ID-X)
 const nNames=cols.filter(c=>!c[0].startsWith('pqv:')).length,nQ=cols.length-nNames;
 const colW=c=>c[0].startsWith('pqv:')?`${(100-22*nNames)/Math.max(1,nQ)}%`:'22%';
 const colgroup=()=>el('colgroup',{},...cols.map(c=>el('col',{style:`width:${colW(c)}`})));
 const headRow=(it)=>{const t=el('table',{class:'pprow'});t.append(colgroup());const tr=el('tr');const s=it.sample;
  for(const [k] of cols){const td=el('td');
   if(!s){if(k==='name'||k==='file')td.textContent=it.overlay&&k===cols[0][0]?T('overlay'):''}
   else if(k==='file')td.textContent=s.file;else if(k==='name')td.textContent=s.name+(it.marker?'   '+it.marker.name:'');else if(k==='panel')td.textContent=s.kit?.panel||s.kit?.name||'';
   else{td.className='pq';const q=k.slice(4);td.innerHTML=pqv(s.pqv?.[q],q!=='SQ'&&sampleEdited(s))}
   tr.append(td)}
  t.append(tr);return t};
 r.pages.forEach((pg,i)=>{const page=el('div',{class:'ppage',style:`width:${r.pw-20}mm;height:${r.ph-20}mm`});
  if(o.header)page.append(el('div',{class:'pphead',style:`height:${r.headH-2}mm`},el('div',{class:'ppapp'},el('b',{text:'OpenTrace'}),el('span',{text:ver})),
   el('div',{class:'ppproj',text:`Project: ${proj}`+(payload.printComment?` — ${payload.printComment}`:'')}),el('div')));
  const ch=el('table',{class:'ppcols'});ch.append(colgroup(),el('tr',{},...cols.map(c=>el('th',{text:c[1]}))));page.append(ch);
  for(const im of pg)page.append(el('div',{class:'ppane',style:`height:${r.paneH-2}mm`},headRow(im),el('img',{src:im.src,style:`width:${r.usableW-2}mm;height:${r.imgH}mm`})));
  page.append(el('div',{class:'ppfoot'},el('span',{text:date}),el('span',{text:`Printed by: ${user}`}),el('span',{text:`Page ${i+1} of ${r.pages.length}`})));
  area.append(page)});
 document.body.append(area);
 if(!preview){document.body.classList.add('printingPlots');const done=()=>{document.body.classList.remove('printingPlots');area.remove();window.removeEventListener('afterprint',done)};
  window.addEventListener('afterprint',done);setTimeout(()=>window.print(),100)}
 return r}
window.PP={printPlots,printPages,pageOpts};
// Zoom To Next Marker (Ctrl+J): the next marker to the right of the current X range, with the marker margin.
function zoomToMarkerDlg(){const cv=[...document.querySelectorAll('canvas')].find(c=>c._geom);if(!cv)return;const G=cv._geom;
 const names=[...new Set(G.markers.map(m=>m.name))];const v=prompt(T('pmZoomToMarkerPrompt')+'\n'+names.join(', '),names[0]||'');if(v==null)return;
 const m=G.markers.find(m=>m.name.toLowerCase()===v.trim().toLowerCase());if(!m){message(T('pmNoMarker').replace('{0}',v));return}
 const mg=setting?.display?.markerMargin??5;let a=m.min_bp-mg,b=m.max_bp+mg;if(G.dp){a=bpToScan(G.curve,a);b=bpToScan(G.curve,b)}
 view.x0=a;view.x1=b;view.userZoom=true;draw()}
function markerMarginDlg(){const cur=setting?.display?.markerMargin??5;const v=prompt(T('pmMarkerMarginPrompt'),String(cur));if(v==null)return;const n=parseFloat(String(v).replace(',','.'));
 if(!(n>=0&&n<=100)||!setting){message(T('pmMarginInvalid'));return}setting.display.markerMargin=n;view.userZoom=false;view.x0=view.x1=null;build();draw()}
function zoomNextMarker(){const cv=[...document.querySelectorAll('canvas')].find(c=>c._geom);if(!cv)return;const G=cv._geom;
 const ms=G.markers.filter(m=>m.channel===G.markers[0]?.channel||true).slice().sort((a,b)=>a.min_bp-b.min_bp);if(!ms.length)return;
 const mid=G.dp?scanToBp(G.curve,(G.x0+G.x1)/2):(G.x0+G.x1)/2;
 const cur=ms.findIndex(m=>mid>=m.min_bp&&mid<=m.max_bp);const n=ms[(cur<0?ms.findIndex(m=>m.min_bp>mid):cur+1)%ms.length]||ms[0];
 const mg=setting?.display?.markerMargin??5;let a=n.min_bp-mg,b=n.max_bp+mg;if(G.dp){a=bpToScan(G.curve,a);b=bpToScan(G.curve,b)}
 view.x0=a;view.x1=b;view.userZoom=true;draw()}
function zoom(f){const [d0,d1]=domain();const a=view.x0??d0,b=view.x1??d1,c=(a+b)/2,w=(b-a)*f/2;view.x0=c-w;view.x1=c+w;view.userZoom=true;draw()}

// ---------- lower: genotypes table + QVD ----------
// Sizing table as in ID-X (SamplePlotTableModel): one row per detected peak of the shown dyes of every shown sample, dyes in
// pane order, peaks by position; "B,3" = 3rd blue peak, "*" = peak matched to the size standard; Marker and Allele only for
// called peaks. A row click selects the peak in the plot (Ctrl/Shift add), a peak selected in the plot highlights its row.
// Table Filter: Show All Rows (Ctrl+F), Show Selected Rows (Ctrl+G) / Hide Selected Rows (Ctrl+H) - the rows selected at that
// moment, Show Allele Call Rows (Ctrl+V). Built once per content; selection changes only re-mark rows.
const DYE_LETTER={DATA1:'B',DATA2:'G',DATA3:'Y',DATA4:'R',DATA105:'O',DATA106:'P'};
let sizing=null;
const escH=v=>String(v??'').replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
function sizingRows(){const out=[];
 for(const s of payload.samples){const a=s.analysis;if(!a)continue;const ref=a.sizing?.reference_channel;const matched=new Set((a.sizing?.matched||[]).map(m=>m.scan));
  for(const tag of DYE_KEYS){if(!view.dyes.has(tag))continue;const ch=(a.channels||[]).find(c=>c.tag===tag);if(!ch)continue;
   const calls=(s.report?.calls||[]).filter(c=>c.channel===tag);let n=0;
   for(const pk of [...(ch.peaks||[])].sort((p,q)=>p.scan-q.scan)){if(pk.size_bp==null)continue;n++;
    const call=calls.find(c=>Math.abs(c.size_bp-pk.size_bp)<0.05);
    out.push({s,tag,pk,n,call,key:peakKey(s,tag,pk),star:tag===ref&&matched.has(pk.scan)})}}}
 return out}
// Rows are virtual: only the visible part of the table is in the DOM (96 samples can have 100 000 peaks).
function renderSizing(){const grid=$('gtGrid');const f=view.tableFilter||'all';
 const sig=[payload,[...view.dyes].join(),f,f==='selected'||f==='hide'?[...(view.filterKeys||[])].join(';'):'',JSON.stringify(setting?.sizingTable||{}),I18N.lang];
 if(!sizing||sizing.sig.length!==sig.length||sizing.sig.some((v,i)=>v!==sig[i])||!grid.contains(sizing.table)){
  const SC=(setting?.sizingTable?.cols)||{};const scols=PlotSettings.SIZING_COLS.filter(([id])=>SC[id]!==false);
  let rows=sizingRows();
  if(f==='selected')rows=rows.filter(r=>view.filterKeys?.has(r.key));else if(f==='hide')rows=rows.filter(r=>!view.filterKeys?.has(r.key));else if(f==='calls')rows=rows.filter(r=>r.call);
  const head=[T('colSampleName'),...scols.map(([id,,h])=>T('psCol_'+id)==='psCol_'+id?h:T('psCol_'+id))];
  const t=el('table',{class:'g sizing'});if(setting)t.style.font=`${setting.sizingTable.size}px ${PlotSettings.cssFont(setting.sizingTable.font)}`;
  t.innerHTML=`<thead><tr>${head.map(h=>`<th>${escH(h)}</th>`).join('')}</tr></thead><tbody></tbody>`;
  grid.replaceChildren(t);grid.scrollTop=0;
  sizing={sig,table:t,body:t.tBodies[0],rows,scols,index:new Map(rows.map((r,i)=>[r.key,i])),rowH:0,from:-1,to:-1,selSig:''};
  t.tBodies[0].addEventListener('mousedown',e=>{const tr=e.target.closest('tr[data-i]');if(!tr)return;const r=sizing.rows[+tr.dataset.i];
   const pp={x:0,y:0,peak:r.pk,tag:r.tag,sample:r.s},multi=e.ctrlKey||e.metaKey||e.shiftKey;
   if(!multi)view.selPeaks.clear();if(multi&&view.selPeaks.has(r.key))view.selPeaks.delete(r.key);else view.selPeaks.set(r.key,pp);
   sizing.fromTable=true;revealPane(r.s.id,r.tag);repaint();showPhr()});
  grid.onscroll=()=>sizingPaint(false)}
 const selSig=[...view.selPeaks.keys()].join(';');const changed=selSig!==sizing.selSig;sizing.selSig=selSig;
 // a peak selected in the plot: bring its row into view
 if(changed&&!sizing.fromTable){const i=[...view.selPeaks.keys()].map(k=>sizing.index.get(k)).filter(i=>i!=null).sort((a,b)=>a-b)[0];
  if(i!=null){sizingPaint(true);const rh=sizing.rowH||18,hh=sizing.table.tHead.offsetHeight,y=hh+i*rh;if(y<grid.scrollTop+hh||y+rh>grid.scrollTop+grid.clientHeight)grid.scrollTop=Math.max(0,y-grid.clientHeight/2)}}
 sizing.fromTable=false;sizingPaint(changed)}
function sizingPaint(force){const S=sizing;if(!S)return;const grid=$('gtGrid');const n=S.rows.length;
 const num=new Set(['size','height','area','dataPoint']);
 const row=i=>{const r=S.rows[i];const col=DYE_COLORS[r.tag]==='#000000'?'#e5d300':DYE_COLORS[r.tag];
  const val={sampleFileName:r.s.file,marker:r.call?.marker||'',dyeSamplePeak:`<span class="dsw" style="background:${col}"></span>${DYE_LETTER[r.tag]||'?'},${r.n}${r.star?' *':''}`,minutes:'',
   size:r.pk.size_bp.toFixed(2),height:String(Math.round(r.pk.height)),area:String(r.pk.area??''),dataPoint:String(Math.round(r.pk.scan)),alleleCall:r.call?String(r.call.label):''};
  return `<tr data-i="${i}"${view.selPeaks.has(r.key)?' class="sel"':''}><td>${escH(r.s.name)}</td>`+S.scols.map(([id])=>`<td${num.has(id)?' class="num"':''}>${id==='dyeSamplePeak'?val[id]:escH(val[id])}</td>`).join('')+'</tr>'};
 if(!S.rowH&&n){S.body.innerHTML=row(0);S.rowH=S.body.rows[0].offsetHeight||18}
 const rh=S.rowH||18,hh=S.table.tHead.offsetHeight;
 const from=Math.max(0,Math.floor((grid.scrollTop-hh)/rh)-30),to=Math.min(n,from+Math.ceil(grid.clientHeight/rh)+60);
 if(!force&&from===S.from&&to===S.to)return;S.from=from;S.to=to;
 const cs=S.scols.length+1,sp=h=>h>0?`<tr class="vsp"><td colspan="${cs}" style="height:${h}px"></td></tr>`:'';
 let h=sp(from*rh);for(let i=from;i<to;i++)h+=row(i);h+=sp((n-to)*rh);S.body.innerHTML=h}
// scroll the plot area to the pane of a sample (and dye) selected in a table
function revealPane(sid,tag){const cv=[...document.querySelectorAll('#plotarea canvas')].find(c=>c._sid===sid&&(!tag||!c._dyes||c._dyes.includes(tag)));
 if(cv){const a=$('plotarea').getBoundingClientRect(),r=cv.getBoundingClientRect();if(r.top<a.top||r.bottom>a.bottom)cv.scrollIntoView({block:'nearest'})}}
function renderLower(){
 $('lower').style.display=view.table==='none'?'none':'flex';$('qvd').style.display=view.table==='sizing'?'none':'';
 const grid=$('gtGrid');if(!payload){grid.replaceChildren();return}
 if(view.table==='sizing'){renderSizing();return}
 sizing=null;grid.onscroll=null;grid.replaceChildren();
 const t=el('table',{class:'g'});
 const cols=[T('colSampleName'),T('colMarker'),T('colAllele')+' 1',T('colAllele')+' 2','GQ'];
 const hr=el('tr');for(const c of cols)hr.append(el('th',{text:c}));t.append(el('thead',{},hr));const tb=el('tbody');
 for(const s of payload.samples){
  const gsel=isGeno()?new Set(payload.genoSel||[]):null;
  for(const m of (s.markers||[]).filter(m=>!gsel||gsel.has(s.id+'|'+m.marker))){const tr=el('tr',{class:view.selMarker===m.marker&&view.selSample===s.id?'sel':''});
   const g=el('td',{class:'ic'});g.innerHTML=pqv(m.gqCat);
   tr.append(el('td',{text:s.name}),el('td',{text:m.marker}),el('td',{text:m.calls[0]?.label||''}),el('td',{text:m.calls[1]?.label||''}),g);
   tr.addEventListener('mousedown',()=>{view.selMarker=m.marker;view.selSample=s.id;revealPane(s.id,((s.kit||payload.kit)?.markers||[]).find(k=>k.name===m.marker)?.channel);renderLower();try{build()}catch(e){}});tb.append(tr)}}
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

// Table Filter: Show / Hide Selected Rows take the rows selected now (ID-X showOnlySelectedRows), Ctrl+V the allele call rows
function tableFilter(f){if(f==='calls'&&view.table!=='sizing')return;view.tableFilter=f;view.filterKeys=f==='selected'||f==='hide'?new Set(view.selPeaks.keys()):null;build();renderLower()}
// ---------- chrome ----------
function build(){
 document.documentElement.lang=I18N.lang;
 document.title=`${T('app')} : ${T(isGeno()?'genotypesPlot':'samplesPlot')}`;
 const set=(k,v)=>()=>{view[k]=v;build();draw()};
 const ck=(k,v)=>view[k]===v;
 const DYE_SC={DATA1:'Ctrl+1',DATA2:'Ctrl+2',DATA3:'Ctrl+3',DATA4:'Ctrl+4',DATA105:'Ctrl+5',DATA106:'Ctrl+0'};
 const selNow=payload?selection():[];
 const dyeItems=DYE_KEYS.map(t=>({label:T('dyes')+': '+DYE_NAMES[t],sc:DYE_SC[t],checked:view.dyes.has(t),run:()=>{view.dyes.has(t)?view.dyes.delete(t):view.dyes.add(t);build();draw()}}));
 buildMenubar($('menubar'),[
  {label:T('file'),items:[{label:T('pageSetup'),run:plotPageSetup},{label:T('print'),sc:'Ctrl+P',run:()=>printPlots(false)},{label:T('printPreview'),run:()=>printPlots(true)},'-',
   {label:T('exportTable'),sc:'Ctrl+E',run:exportPlotTable},{label:T('exportWithStutter')+'...',sc:'Ctrl+S',run:()=>{try{window.opener.OT.exportWithStutter(payload.samples.map(s=>s.id));window.opener.focus()}catch(e){message(String(e.message||e))}}},'-',{label:T('closePlot'),sc:'Esc',run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('undo'),sc:'Ctrl+Z',run:undoLast},{label:T('copy'),sc:'Ctrl+C',run:copyPlotTable}]},
  {label:T('view'),items:[{label:T('psToolbar'),checked:view.toolbar,run:set('toolbar',!view.toolbar)},'-',
   {label:T('sampleInfo'),sc:'Ctrl+F1',run:()=>sampleView('info')},{label:T('rawData'),sc:'Ctrl+F2',run:()=>sampleView('raw')},{label:T('eptData'),sc:'Ctrl+F3',run:()=>sampleView('ept')},'-',
   {label:T('language'),sub:[{label:'Русский',checked:I18N.lang==='ru',run:()=>{I18N.set('ru');build();draw()}},{label:'English',checked:I18N.lang==='en',run:()=>{I18N.set('en');build();draw()}}]}]},
  {label:T('plots'),items:[
   {label:T('zoom'),sub:[{label:T('fullView'),sc:'Ctrl+]',run:xFullView},{label:T('zoomIn'),sc:'Ctrl+=',run:()=>zoom(.5)},{label:T('zoomOut'),sc:'Ctrl+-',run:()=>zoom(2)},{label:T('zoomTo'),run:xZoomToDlg},'-',
    {label:T('pmZoomToMarker'),run:zoomToMarkerDlg},{label:T('zoomToNextMarker'),sc:'Ctrl+J',run:zoomNextMarker},{label:T('pmRetainX'),checked:!!view.retainX,run:set('retainX',!view.retainX)}]},
   {label:T('xAxis'),sub:[{label:T('basepairs'),sc:'Ctrl+Y',checked:view.xAxis==='bp',run:()=>xAxisTo('bp')},{label:T('dataPoints'),sc:'Ctrl+N',checked:view.xAxis==='dp',run:()=>xAxisTo('dp')},'-',{label:T('zoomTo'),sc:'Ctrl+Shift+G',run:xZoomToDlg}]},
   {label:T('yAxis'),sub:[{label:T('scaleMaxY'),sc:'Ctrl+Shift+M',checked:view.yAxis==='max',run:()=>yAxisTo('max')},{label:T('scaleInd'),sc:'Ctrl+Shift+D',checked:view.yAxis==='individual',run:()=>yAxisTo('individual')},
    {label:T('psScaleEntered')+` (${view.yValue})...`,sc:'Ctrl+Shift+Y',checked:view.yAxis==='entered',run:yZoomToDlg}]},'-',
   {label:T('labels'),sub:[{label:T('noLabels'),sc:'Ctrl+Shift+N',checked:ck('labels','none'),run:set('labels','none')},{label:T('hLabels'),sc:'Ctrl+Shift+H',checked:ck('labels','h'),run:set('labels','h')},{label:T('vLabels'),sc:'Ctrl+Shift+V',checked:ck('labels','v'),run:set('labels','v')}]},
   {label:T('pmSizeStd'),checked:view.sizeStdLabels,run:set('sizeStdLabels',!view.sizeStdLabels)},
   {label:T('showBins'),sc:'Ctrl+Shift+B',checked:view.bins,run:set('bins',!view.bins)},
   {label:T('psPeakPositions'),checked:view.peakPositions,run:()=>{view.peakPositions=!view.peakPositions;build();draw()}},
   {label:T('pmAlleleChanges'),checked:view.alleleChanges,run:set('alleleChanges',!view.alleleChanges)},
   {label:T('pmControlsTop'),checked:view.controlsTop,run:set('controlsTop',!view.controlsTop)},
   {label:T('pmLaddersTop'),checked:view.laddersTop,run:set('laddersTop',!view.laddersTop)},
   {label:T('pmOffscale'),checked:view.offscale,run:set('offscale',!view.offscale)},'-',
   {label:T('dyes'),sub:[...dyeItems.map((d,i)=>({...d,label:DYE_NAMES[DYE_KEYS[i]]})),'-',{label:T('allDyes'),sc:'Ctrl+6',run:()=>{DYE_KEYS.forEach(t=>view.dyes.add(t));build();draw()}}]},
   {label:T('plots'),sub:[{label:T('combine'),sc:'Ctrl+7',checked:ck('layout','combine'),run:set('layout','combine')},{label:T('separate'),sc:'Ctrl+8',checked:ck('layout','separate'),run:set('layout','separate')},{label:T('overlay'),sc:'Ctrl+9',checked:ck('layout','overlay'),run:set('layout','overlay')}]},
   {label:T('pmTableFilter'),sub:[{label:T('pmShowAllRows'),sc:'Ctrl+F',checked:(view.tableFilter||'all')==='all',run:()=>tableFilter('all')},{label:T('pmShowSelRows'),sc:'Ctrl+G',checked:view.tableFilter==='selected',run:()=>tableFilter('selected')},
    {label:T('pmHideSelRows'),sc:'Ctrl+H',checked:view.tableFilter==='hide',run:()=>tableFilter('hide')},{label:T('pmShowCallRows'),sc:'Ctrl+V',disabled:view.table!=='sizing',checked:view.tableFilter==='calls',run:()=>tableFilter('calls')}]},
   {label:T('pmTables'),sub:[{label:T('noTable'),sc:'Ctrl+Q',checked:ck('table','none'),run:set('table','none')},{label:T('sizingTable'),sc:'Ctrl+A',checked:ck('table','sizing'),run:set('table','sizing')},{label:T('genotypesTable'),sc:'Ctrl+W',checked:ck('table','genotypes'),run:set('table','genotypes')}]},'-',
   {label:T('showHeader'),checked:view.header,run:set('header',!view.header)},{label:T('markerRange'),sc:'Ctrl+R',checked:view.markerRange,run:set('markerRange',!view.markerRange)},
   {label:T('markerIndicator'),sc:'Ctrl+K',checked:view.markerIndicator,run:()=>{view.markerIndicator=!view.markerIndicator;build();draw()}},
   {label:T('pmPqvTrigger'),checked:!!setting?.labels?.pqvTrigger,run:()=>{if(setting?.labels){setting.labels.pqvTrigger=!setting.labels.pqvTrigger;build();draw()}}},
   {label:T('psPlotColors'),sub:[{label:T('psStandardColors'),checked:!view.customColors,run:()=>{view.customColors=false;build();draw()}},{label:T('psCustom'),checked:view.customColors,run:()=>{view.customColors=true;build();draw()}}]},'-',
   {label:T('pmLegend'),checked:!!view.legend,run:set('legend',!view.legend)},
   {label:T('pmMarkerMargin'),run:markerMarginDlg},'-',
   {label:T('multipleDyePlot'),run:multipleDyePlot}]},
  {label:T('tools'),items:[{label:T('manager'),disabled:true},'-',{label:T('psEditor'),sc:'Ctrl+T',run:editSetting}]},
  {label:T('alleles'),items:[
   {label:T('editingMode'),sub:[{label:T('peakSelection'),sc:'Ctrl+;',checked:!singleClickOn(),run:()=>setSingleClick(false)},
    {label:T('singleClickEditing'),checked:singleClickOn(),disabled:!singleClickAllowed(),run:()=>setSingleClick(!view.singleClick)}]},'-',
   {label:T('deletePredefined')+'...',run:deletePredefined},'-',
   {label:T('addAlleleLabel'),sc:'Ctrl+L',disabled:!selNow.some(p=>!p.label&&p.marker),run:addAlleleSelected},
   {label:T('addArtifactLabel'),sc:'Ctrl+I',disabled:!selNow.some(p=>!p.label?.artifact),run:()=>addArtifactSelected()},
   {label:T('deleteLabels'),sc:'Del',disabled:!selNow.some(p=>p.label),run:deleteSelected},
   {label:T('deleteMultiple'),sub:[{label:T('deleteAllLabels'),run:()=>deleteLabels(payload?.samples||[],null)},{label:T('deleteInRange')+'...',run:deleteInRangeDialog}]},
   {label:T('renameAllele'),disabled:!selNow.some(p=>p.label?.call),sub:selNow.some(p=>p.label?.call)?renameAlleleItems():undefined},
   {label:T('renameArtifactMenu'),disabled:!selNow.some(p=>p.label?.artifact),sub:selNow.some(p=>p.label?.artifact)?renameArtifactItems():undefined},'-',
   {label:T('history'),run:openHistory},{label:T('editHowTo'),run:()=>message(T('editHint'))}]},
  {label:T('help'),items:[{label:T('about'),run:()=>message(T('aboutText'),T('about'))}]}]);
 const t1=$('toolbar1');t1.replaceChildren(tbutton('print',T('print'),()=>printPlots(false),{id:'tbPlotPrint'}),tbutton('printPreview',T('printPreview'),()=>printPlots(true),{id:'tbPlotPreview'}),tsep(),el('span',{class:'tlabel',text:T('plotSetting')}));
 const ps=el('select',{class:'combo',id:'psCombo'});for(const n of settingNames())ps.append(new Option(n,n));ps.value=setting?.name||'';
 ps.addEventListener('change',()=>{applySetting(ps.value);build();draw()});
 const pn=el('select',{class:'combo',style:'min-width:44px'});for(let i=1;i<=8;i++)pn.append(new Option(String(i),String(i)));pn.value=String(view.panes);
 pn.addEventListener('change',()=>{view.panes=+pn.value;draw()});
 t1.append(ps,tbutton('plotSettingEditor',T('psEditor'),editSetting,{id:'tbPlotSettingEditor'}),tsep(),el('span',{class:'tlabel',text:T('panes')}),pn,tsep());
 for(const tag of DYE_KEYS){const b=el('button',{class:'tb'+(view.dyes.has(tag)?' on':''),type:'button',title:DYE_NAMES[tag]});
  b.innerHTML=`<svg viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12" fill="${DYE_COLORS[tag]==='#000000'?'#e5d300':DYE_COLORS[tag]}" stroke="#333"/></svg>`;
  b.addEventListener('click',()=>{view.dyes.has(tag)?view.dyes.delete(tag):view.dyes.add(tag);build();draw()});t1.append(b)}
 const tg=(name,title,on,run)=>{const b=tbutton(name,title,run);if(on)b.classList.add('on');return b};
 const t2=$('toolbar2');t2.replaceChildren(
  tg('header',T('showHeader'),view.header,set('header',!view.header)),tg('markerRange',T('markerRange'),view.markerRange,set('markerRange',!view.markerRange)),tsep(),
  tg('noLabels',T('noLabels'),ck('labels','none'),set('labels','none')),tg('hLabels',T('hLabels'),ck('labels','h'),set('labels','h')),tg('vLabels',T('vLabels'),ck('labels','v'),set('labels','v')),
  tg('bins',T('showBins'),view.bins,set('bins',!view.bins)),tsep(),tbutton('fullView',T('fullView'),()=>{view.x0=view.x1=null;draw()}),tsep(),
  tg('combine',T('combine'),ck('layout','combine'),set('layout','combine')),tg('separate',T('separate'),ck('layout','separate'),set('layout','separate')),tg('overlay',T('overlay'),ck('layout','overlay'),set('layout','overlay')),tsep(),
  tbutton('combine',T('multipleDyePlot'),multipleDyePlot,{id:'tbMultiDye'}),tsep(),
  tg('noTable',T('noTable'),ck('table','none'),set('table','none')),tg('sizingTable',T('sizingTable'),ck('table','sizing'),set('table','sizing')),tg('genotypesTable',T('genotypesTable'),ck('table','genotypes'),set('table','genotypes')));
 t2.style.display=view.toolbar?'':'none';
 renderEditBar();
 const H=(isGeno()?setting?.genotypeHeader:setting?.sampleHeader)||{};const on=k=>H[k]!==false;
 const ch=$('colhead');if(isGeno()){ch.replaceChildren(...(on('sampleFileName')?[el('div',{class:'c',style:'width:250px',text:T('colSampleFile')})]:[]),
  ...(on('sampleName')?[el('div',{class:'c',style:'width:150px',text:T('colSampleName')})]:[]),...(on('panel')?[el('div',{class:'c',style:'width:150px',text:T('colPanel')})]:[]),
  ...(on('marker')?[el('div',{class:'c',style:'width:100px',text:T('colMarker')})]:[]),...GENO_PQV.filter(on).map(k=>el('div',{class:'c',style:'width:46px',text:k})));ch.style.display=view.header?'flex':'none'}
 else ch.replaceChildren(...(on('sampleFileName')?[el('div',{class:'c',style:'width:250px',text:T('colSampleFile')})]:[]),
  ...(on('sampleName')?[el('div',{class:'c',style:'width:150px',text:T('colSampleName')})]:[]),...(on('panel')?[el('div',{class:'c',style:'width:150px',text:T('colPanel')})]:[]),
  ...PQV_KEYS.filter(on).map(k=>el('div',{class:'c',style:'width:46px',text:k})));if(!isGeno())ch.style.display=view.header?'flex':'none';
}
// Hot keys follow ID-X PlotMgrPane (docs: IDX_UI_HOTKEYS.md): Ctrl+1..5,0 toggle a dye, Ctrl+6 all dyes, Ctrl+7/8/9 combine/separate/overlay,
// Ctrl+] full view, Ctrl+R marker range, Ctrl+Shift+B bins, Ctrl+F1/F2/F3 sample views, Ctrl+P/E/Z/+/-.
const DYE_BY_DIGIT={Digit1:'DATA1',Digit2:'DATA2',Digit3:'DATA3',Digit4:'DATA4',Digit5:'DATA105',Digit0:'DATA106'};
const LAYOUT_BY_DIGIT={Digit7:'combine',Digit8:'separate',Digit9:'overlay'};
document.addEventListener('keydown',e=>{if(!(e.ctrlKey||e.metaKey))return;
 const inField=!!e.target.closest('input,textarea,select');const k=hotKey(e);
 // digit keys by position; input without a scan code (some remote/virtual keyboards) has an empty e.code
 const code=e.code||(/^[0-9]$/.test(e.key)?'Digit'+e.key:'');
 // Ctrl+Shift: B bins; M / D / Y Y axis to maximum / individually / to a value; G X axis to a range; N / H / V no / horizontal / vertical labels
 if(e.shiftKey&&!e.altKey){if(inField)return;const act={KeyB:()=>{view.bins=!view.bins;build();draw()},KeyM:()=>yAxisTo('max'),KeyD:()=>yAxisTo('individual'),KeyY:yZoomToDlg,KeyG:xZoomToDlg,
   KeyN:()=>{view.labels='none';build();draw()},KeyH:()=>{view.labels='h';build();draw()},KeyV:()=>{view.labels='v';build();draw()}}[e.code];
  if(act){e.preventDefault();act()}return}
 if(e.altKey)return;
 if(DYE_BY_DIGIT[code]&&!inField){e.preventDefault();const t=DYE_BY_DIGIT[code];view.dyes.has(t)?view.dyes.delete(t):view.dyes.add(t);build();draw();return}
 if(code==='Digit6'&&!inField){e.preventDefault();DYE_KEYS.forEach(t=>view.dyes.add(t));build();draw();return}
 if(LAYOUT_BY_DIGIT[code]&&!inField){e.preventDefault();view.layout=LAYOUT_BY_DIGIT[code];build();draw();return}
 if(e.key===']'&&!inField){e.preventDefault();view.x0=view.x1=null;draw();return}
 if(/^F[123]$/.test(e.key)&&!inField){e.preventDefault();sampleView({F1:'info',F2:'raw',F3:'ept'}[e.key]);return}
 if(k==='j'&&!inField){e.preventDefault();zoomNextMarker();return}
 // Ctrl+Y / Ctrl+N: X axis in basepairs / data points; Ctrl+Q / Ctrl+A / Ctrl+W: no table / sizing table / genotypes table (ID-X PlotMgrPane)
 if(k==='y'&&!inField){e.preventDefault();xAxisTo('bp');return}
 if(k==='n'&&!inField){e.preventDefault();xAxisTo('dp');return}
 if({q:1,a:1,w:1}[k]&&!inField){e.preventDefault();view.table={q:'none',a:'sizing',w:'genotypes'}[k];if(view.table!=='sizing'&&view.tableFilter==='calls')view.tableFilter='all';build();draw();return}
 if(k==='l'&&!inField){e.preventDefault();addAlleleSelected();return}
 if(k==='i'&&!inField){e.preventDefault();addArtifactSelected();return}
 if(e.code==='Semicolon'&&!inField){e.preventDefault();setSingleClick(false);return}
 if(k==='r'&&!inField){e.preventDefault();view.markerRange=!view.markerRange;build();draw();return}
 if(k==='t'&&!inField){e.preventDefault();editSetting();return}
 if(k==='k'&&!inField){e.preventDefault();view.markerIndicator=!view.markerIndicator;build();draw();return}
 if(k==='s'){e.preventDefault();try{window.opener.OT.exportWithStutter(payload.samples.map(s=>s.id));window.opener.focus()}catch(err){message(String(err.message||err))}return}
 if(e.code==='NumpadAdd'){e.preventDefault();zoom(.5);return}
 if(e.code==='NumpadSubtract'){e.preventDefault();zoom(2);return}
 if(!inField&&{f:'all',g:'selected',h:'hide',v:'calls'}[k]){e.preventDefault();tableFilter({f:'all',g:'selected',h:'hide',v:'calls'}[k]);return}
 if(k==='p'){e.preventDefault();printPlots(false);return}
 if(k==='z'&&!inField){e.preventDefault();undoLast();return}
 if(k==='e'){e.preventDefault();exportPlotTable();return}
 if(e.key==='='||e.key==='+'){e.preventDefault();zoom(.5)}
 if(e.key==='-'){e.preventDefault();zoom(2)}});
// Delete: Delete Selected Label(s)
document.addEventListener('keydown',e=>{if(e.key!=='Delete'||e.ctrlKey||e.altKey||e.metaKey||e.target.closest('input,textarea,select')||document.querySelector('.dlg,.backdrop'))return;
 e.preventDefault();deleteSelected()});
let escBusy=false;document.addEventListener('keydown',e=>{if(e.key==='Escape'){escBusy=!!document.querySelector('.dlg,.backdrop,.menu.show');document.querySelectorAll('.menu.ctx').forEach(m=>m.remove())}},true);
document.addEventListener('keydown',e=>{if(e.key!=='Escape'||e.defaultPrevented||e.ctrlKey||e.altKey||e.shiftKey)return;
 if(escBusy)return;if(e.target.closest('input,textarea,select'))return;
 if(view.selPeaks&&view.selPeaks.size){view.selPeaks.clear();repaint();showPhr();return}
 if(window.opener)window.close()});
window.addEventListener('resize',()=>draw());
// Follow the project window's selection (Project Options: update plots when the selection changes).
function refresh(){const sig=p=>JSON.stringify([p?.mode,p?.genoSel,p?.samples?.map(s=>s.id)]);const before=sig(payload);let next=null;try{next=window.opener?.OT?.plotPayload()}catch(e){}
 if(next&&sig(next)!==before&&next.samples.length){payload=next;view.x0=view.x1=null;build();draw()}
 else if(next){payload.project=next.project;payload.printComment=next.printComment;printHead()}}
window.otRefresh=refresh;
window.PV={view,get setting(){return setting}};
window.addEventListener('focus',refresh);
function printHead(){const h=document.getElementById('printHead');if(!h)return;
 h.textContent=[payload?.project||'',payload?.printComment||''].filter(Boolean).join(' — ')}
window.addEventListener('beforeprint',printHead);
window.addEventListener('opentrace-plotsettings',()=>build());
window.addEventListener('storage',e=>{if(e.key===PlotSettings.KEY){const n=setting?.name;if(n&&PlotSettings.get(n))setting=PlotSettings.get(n);build();draw()}});
load();
})();
