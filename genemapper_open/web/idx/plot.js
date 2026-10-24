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
 x0:null,x1:null,selMarker:null,selSample:null,xAxis:'bp',yAxis:'individual',yValue:1000,peakPositions:false,offscale:true,toolbar:true,
 alleleChanges:true,sizeStdLabels:false,controlsTop:false,laddersTop:false,customColors:false};
// ---------- plot settings (Plot Settings Editor) ----------
const PS_ACTIVE='opentrace.plotSettings.active';
const NAME_DYE={blue:'DATA1',green:'DATA2',yellow:'DATA3',red:'DATA4',purple:'DATA105',orange:'DATA106'};
let setting=null;
function settingNames(){return PlotSettings.list().map(r=>r.setting.name).filter(n=>!window.Sec||!Sec.items||Sec.items.can('plots',n,'read'))}
function activeName(){let n='';try{n=localStorage.getItem(PS_ACTIVE)||''}catch(e){}const names=settingNames();return names.includes(n)?n:(names.includes('Data Interpretation')?'Data Interpretation':names[0])}
function applySetting(name){setting=PlotSettings.get(name)||PlotSettings.factory()[0];try{localStorage.setItem(PS_ACTIVE,setting.name)}catch(e){}
 const D=setting.display.persistence==='last'&&setting.lastView?{...setting.display,...setting.lastView}:setting.display;
 Object.assign(view,{panes:D.panes,layout:D.layout,labels:D.labels,bins:D.bins,header:D.header,markerRange:D.markerRange,table:D.table==='lev'?'none':D.table,
  xAxis:D.xAxis,yAxis:D.yAxis,yValue:D.yValue,peakPositions:D.peakPositions,offscale:D.offscale,toolbar:D.toolbar,alleleChanges:D.alleleChanges,
  sizeStdLabels:D.sizeStdLabels,controlsTop:D.controlsTop,laddersTop:D.laddersTop,x0:null,x1:null});
 view.dyes=new Set(Object.entries(D.dyes).filter(([,on])=>on).map(([d])=>NAME_DYE[d]));
 view.userZoom=false;if(!D.allDyes&&D.xAxis==='bp'&&D.end>D.start){view.x0=D.start;view.x1=D.end}
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
 const area=$('plotarea');area.replaceChildren();
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
   for(const k of PQV_KEYS.filter(on)){const c=el('div',{class:'c',style:'width:46px;justify-content:center'});c.innerHTML=p.overlay?'':pqv(s.pqv?.[k]);head.append(c)}
   if(!p.overlay){const lab=el('label',{class:'c',style:'gap:4px'});lab.append(el('input',{type:'checkbox'}),T('markSample'));head.append(lab);head._sample=s;head.title=T('headerMenuHint')}
   pane.append(head)}
  const canvas=el('canvas');canvas.height=h;pane.append(canvas);area.append(pane);
  // the Genotypes plot shows the marker size range plus the Marker Margin of the plot setting
  let px0=x0,px1=x1;if(p.marker&&!view.userZoom){const mg=setting?.display?.markerMargin??5,c=curveOf(s.analysis);
   px0=p.marker.min_bp-mg;px1=p.marker.max_bp+mg;if(view.xAxis==='dp'){px0=bpToScan(c,px0);px1=bpToScan(c,px1)}}
  requestAnimationFrame(()=>paint(canvas,items,px0,px1,p.marker))}
 renderLower()}

function paint(cv,items,x0,x1,onlyMarker){
 const dpr=window.devicePixelRatio||1,W=cv.clientWidth,H=cv.height;cv.width=W*dpr;cv.height=H*dpr;cv.style.height=H+'px';
 const g=cv.getContext('2d');g.scale(dpr,dpr);g.fillStyle='#fff';g.fillRect(0,0,W,H);g.font='11px Tahoma,Arial';
 const LS=setting?.labels||PlotSettings.base('').labels;const fs=LS.size||11;const lineH=fs+3;
 const maxLines=Math.max(1,...['assigned','custom','ladder','artifact'].map(k=>{const i=LS[k].indexOf('none');return i<0?4:Math.max(1,i)}));const labelH=maxLines*lineH+3;
 const L=50,R=12,top=view.markerRange?36:16,bottom=H-(view.labels==='none'?16:10+2*(labelH+2));
 const s=items[0].sample;const cur=curveOf(s.analysis);const dp=view.xAxis==='dp';
 const pos=(bp,scan)=>dp?(scan??bpToScan(cur,bp)):bp;   // plot coordinate of a peak / bin edge
 const Xc=v=>L+(v-x0)/(x1-x0)*(W-L-R);const X=bp=>Xc(dp?bpToScan(cur,bp):bp);
 const markers=((s.kit||payload.kit)?.markers||[]).filter(m=>!onlyMarker||m.name===onlyMarker.name);
 const dyes=items.flatMap(i=>i.dyes);
 // marker headers + bins
 for(const m of markers.filter(m=>dyes.includes(m.channel))){
  const row=(s.markers||[]).find(r=>r.marker===m.name);const xa=X(m.min_bp),xb=X(m.max_bp);
  // marker header colour = GQ; grey when its allele labels were edited
  if(view.markerRange){g.fillStyle=row?.edited?'#a8a8a8':GQ_FILL[row?.gqCat||'na'];g.fillRect(xa,4,xb-xa,14);g.strokeStyle='#555';g.strokeRect(xa+.5,4.5,xb-xa-1,13);
   g.fillStyle='#000';g.save();g.beginPath();g.rect(xa,4,xb-xa,14);g.clip();g.fillText((row?.pli?'** ':'')+m.name,xa+3,15);g.restore();
   m._hit={x0:xa,x1:xb,y0:4,y1:18}}
  if(view.bins)for(const b of m.bins||[]){const ba=X(b.center_bp-(b.left_bp??.5)),bb=X(b.center_bp+(b.right_bp??.5));
   g.fillStyle=b.virtual?'rgba(255,150,190,.35)':'rgba(150,150,150,.28)';g.fillRect(ba,top,Math.max(1,bb-ba),bottom-top)}}
 // axes
 g.strokeStyle='#000';g.beginPath();g.moveTo(L,top);g.lineTo(L,bottom);g.lineTo(W-R,bottom);g.stroke();
 const step=niceStep((x1-x0)/8);g.fillStyle='#000';g.textAlign='center';
 for(let v=Math.ceil(x0/step)*step;v<=x1;v+=step){const x=Xc(v);g.fillText(String(+v.toFixed(1)),x,top-3);g.beginPath();g.moveTo(x,top);g.lineTo(x,top+3);g.stroke()}
 // traces
 let ymax=1;const traces=[];
 for(const it of items)for(const t of traceData(it,x0,x1)){ymax=Math.max(ymax,t.max);traces.push(t)}
 if(!traces.length){g.fillStyle='#777';g.textAlign='center';g.fillText(T('noValues'),W/2,(top+bottom)/2);return}
 // Y axis: scale individually, to the maximum Y of all panes, or to the entered value
 ymax=view.yAxis==='entered'?view.yValue:view.yAxis==='max'&&view.yGlobal?view.yGlobal*1.08:ymax*1.08;const Y=v=>bottom-(Math.min(v,ymax*1.2)/ymax)*(bottom-top);
 g.textAlign='right';const ys=niceStep(ymax/4);for(let v=0;v<=ymax;v+=ys){const y=Y(v);g.fillText(String(Math.round(v)),L-4,y+4);g.beginPath();g.moveTo(L-3,y);g.lineTo(L,y);g.stroke()}
 g.save();g.beginPath();g.rect(L,top,W-L-R,bottom-top);g.clip();
 // off-scale data: pink band (Display Settings "Off-scale data")
 if(view.offscale)for(const t of traces)for(const pk of t.peaks)if(pk.offscaled&&pk.size_bp!=null){const a=Xc(pos(pk.size_bp,pk.begin??pk.scan-3)),b=Xc(pos(pk.size_bp,pk.end??pk.scan+3));
  const xa=dp?a:X(pk.size_bp)-3,xb=dp?b:X(pk.size_bp)+3;g.fillStyle='rgba(255,105,180,.3)';g.fillRect(Math.min(xa,xb),top,Math.max(2,Math.abs(xb-xa)),bottom-top)}
 traces.forEach((t,ti)=>{g.strokeStyle=view.customColors&&setting?setting.display.customColors[ti%setting.display.customColors.length]:(DYE_COLORS[t.tag]||'#333');
  g.lineWidth=1;g.beginPath();t.pts.forEach(([x,v],i)=>i?g.lineTo(Xc(x),Y(v)):g.moveTo(Xc(x),Y(v)));g.stroke()});
 // peak position indicators: a tick under each detected peak
 if(view.peakPositions)for(const t of traces)for(const pk of t.peaks){if(pk.size_bp==null&&!dp)continue;const x=Xc(pos(pk.size_bp,pk.scan));if(x<L||x>W-R)continue;
  g.strokeStyle=DYE_COLORS[t.tag]||'#333';g.beginPath();g.moveTo(x,bottom-1);g.lineTo(x,bottom-7);g.stroke()}
 // size-standard labels on the reference dye
 if(view.sizeStdLabels){g.textAlign='center';g.font=`${fs}px ${LS.font||'Tahoma'}`;for(const t of traces){if(t.tag!==t.sample.analysis?.sizing?.reference_channel)continue;
  for(const m of t.sample.analysis?.sizing?.matched||[]){const mbp=m.expected_bp??m.bp;if(m.scan==null||mbp==null)continue;const v=dp?m.scan:mbp;if(v<x0||v>x1)continue;
   g.fillStyle='#000';g.fillText(String(+(+mbp).toFixed(1)),Xc(v),Math.max(top+fs,Y(m.height||0)-4))}}}
 g.restore();
 // allele labels in boxes under the axis
 const labelBoxes=[];let noRoom=false;
 if(view.labels!=='none'){g.textAlign='center';const placed=[];g.font=`${fs}px ${LS.font||'Tahoma'}`;
  for(const t of traces){const calls=(t.sample.report?.calls||[]).filter(c=>c.channel===t.tag&&(dp?c.scan>=x0&&c.scan<=x1:c.size_bp>=x0&&c.size_bp<=x1));
   const dyeCol=DYE_COLORS[t.tag]==='#000000'?'#7a6a00':(DYE_COLORS[t.tag]||'#000');
   for(const c of calls){const x=Xc(pos(c.size_bp,c.scan));const lines=labelLines(c,t.sample);
    const w=Math.max(18,...lines.map(l=>g.measureText(l).width+8));let row=0;while(placed.some(p=>p.row===row&&Math.abs(p.x-x)<(p.w+w)/2+2))row++;if(row>1){noRoom=true;continue}placed.push({x,row,w});
    const y=bottom+6+row*(labelH+2);const h=lines.length*lineH+2;
    const border=(c.flags||[]).length?'#e040a0':LS.color==='bw'?'#000':dyeCol;g.strokeStyle=border;g.fillStyle='#fff';g.fillRect(x-w/2,y,w,h);g.strokeRect(x-w/2+.5,y+.5,w-1,h-1);
    g.fillStyle=view.alleleChanges&&c.user_edited?'#b00060':LS.color==='dyeAll'?dyeCol:'#000';lines.forEach((l,i)=>g.fillText(l,x,y+lineH*(i+1)-2));
    g.beginPath();g.moveTo(x,bottom);g.lineTo(x,y);g.strokeStyle='#999';g.stroke();
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
 if(noRoom){g.save();g.font='italic 11px Tahoma,Arial';g.textAlign='right';g.fillStyle='#b00000';g.fillText(T('noRoomLabels'),W-R-4,H-4);g.restore()}
 const peakPts=[];for(const t of traces)for(const pk of t.peaks){if(pk.size_bp==null)continue;const v=pos(pk.size_bp,pk.scan);if(v>=x0&&v<=x1)peakPts.push({x:Xc(v),y:Y(pk.height),peak:pk,tag:t.tag,sample:t.sample})}
 // selected peaks (ID-X: "the peak fills in when it is selected")
 for(const pp of peakPts){if(!view.selPeaks.has(peakKey(pp.sample,pp.tag,pp.peak)))continue;const col=DYE_COLORS[pp.tag]||'#333';
  g.save();g.globalAlpha=.35;g.fillStyle=col;g.beginPath();g.moveTo(pp.x-4,bottom);g.lineTo(pp.x,pp.y);g.lineTo(pp.x+4,bottom);g.closePath();g.fill();g.restore();
  g.fillStyle=col;g.beginPath();g.arc(pp.x,pp.y,4,0,7);g.fill();g.strokeStyle='#000';g.stroke()}
 cv._geom={X,x0,x1,L,R,W,top,bottom,ymax,dp,curve:cur,noRoom,markers:markers.filter(m=>dyes.includes(m.channel)),sample:s,labelBoxes,peakPts};
}
// trace points of one pane item in the current coordinate (bp or data points)
function traceData(it,x0,x1){const a=it.sample.analysis,c=curveOf(a),out=[];
 for(const tag of it.dyes){const ch=(a?.channels||[]).find(q=>q.tag===tag);const values=ch?.analyzed_values||ch?.values;if(!values)continue;
  const pts=[];let max=1;values.forEach((v,i)=>{const x=view.xAxis==='dp'?i:scanToBp(c,i);if(x>=x0&&x<=x1){pts.push([x,v]);if(v>max)max=v}});
  out.push({tag,pts,max,peaks:ch.peaks||[],sample:it.sample})}
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
view.selPeaks=view.selPeaks||new Map();
const nearPeak=(G,x)=>G.peakPts.filter(p=>Math.abs(p.x-x)<=6).sort((a,b)=>Math.abs(a.x-x)-Math.abs(b.x-x))[0];
document.addEventListener('mousedown',e=>{const cv=e.target.closest('canvas');if(!cv?._geom)return;const r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top,G=cv._geom;
 const m=G.markers.find(m=>m._hit&&x>=m._hit.x0&&x<=m._hit.x1&&y>=m._hit.y0&&y<=m._hit.y1);
 if(m){view.selMarker=m.name;view.selSample=G.sample.id;renderLower();return}
 if(y<G.top+4){drag={cv,x0:x};e.preventDefault();return}
 if(e.button!==0||y>G.bottom)return;
 const near=nearPeak(G,x);const multi=e.ctrlKey||e.metaKey||e.shiftKey;
 if(!near){if(!multi&&view.selPeaks.size){view.selPeaks.clear();draw()}return}
 const k=peakKey(near.sample,near.tag,near.peak);if(!multi){const had=view.selPeaks.has(k)&&view.selPeaks.size===1;view.selPeaks.clear();if(!had)view.selPeaks.set(k,near)}
 else if(view.selPeaks.has(k))view.selPeaks.delete(k);else view.selPeaks.set(k,near);draw();showPhr()});
// Calculating Peak Height Ratio: Ctrl+click two peaks, the ratio (smaller / larger height) is shown in the status bar
function showPhr(){const c=$('phr');if(!c)return;const sel=[...view.selPeaks.values()];
 if(sel.length!==2){c.textContent='';return}const [a,b]=sel.map(p=>p.peak.height);c.textContent=fmt('phrStatus',(Math.min(a,b)/Math.max(a,b)).toFixed(2),Math.round(a),Math.round(b))}
document.addEventListener('mouseup',e=>{if(!drag)return;const G=drag.cv._geom,r=drag.cv.getBoundingClientRect(),x=e.clientX-r.left;
 const inv=px=>G.x0+(px-G.L)/(G.W-G.L-G.R)*(G.x1-G.x0);const a=inv(Math.min(drag.x0,x)),b=inv(Math.max(drag.x0,x));drag=null;
 if(b-a>1){view.x0=a;view.x1=b;view.userZoom=true;draw()}});
document.addEventListener('dblclick',e=>{if(e.target.closest('canvas')){view.x0=view.x1=null;view.userZoom=false;draw()}});
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
function ctxMenu(x,y,items){document.querySelectorAll('.menu.ctx').forEach(m=>m.remove());
 const m=el('div',{class:'menu ctx show',style:`left:${x}px;top:${y}px`});
 for(const it of items){if(it==='-'){m.append(el('div',{class:'msep'}));continue}const r=el('div',{class:'mi'+(it.disabled?' dis':''),text:it.label});if(!it.disabled)r.addEventListener('click',()=>{m.remove();it.run()});m.append(r)}
 document.body.append(m);setTimeout(()=>document.addEventListener('mousedown',function h(e){if(!m.contains(e.target)){m.remove();document.removeEventListener('mousedown',h)}}),0)}
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
 if(ok){try{payload=window.opener.OT.plotPayload()}catch(e){}build();draw()}}
function binLabel(marker,bp){const b=(marker.bins||[]).find(b=>bp>=b.center_bp-(b.left_bp??.5)&&bp<=b.center_bp+(b.right_bp??.5));return b?b.label:'OL'}
document.addEventListener('contextmenu',e=>{const cv=e.target.closest('canvas');const head=e.target.closest('.phead');
 if(head&&head._sample){e.preventDefault();const s=head._sample;if(!s.editable)return;
  ctxMenu(e.clientX,e.clientY,[{label:T('overrideCgq'),run:()=>edit(s,{action:'override_cgq'})}]);return}
 if(!cv?._geom)return;e.preventDefault();const G=cv._geom,r=cv.getBoundingClientRect(),x=e.clientX-r.left,y=e.clientY-r.top;
 const s=G.sample;if(!s.editable)return;
 const box=G.labelBoxes.find(b=>x>=b.x0&&x<=b.x1&&y>=b.y0&&y<=b.y1);
 if(box&&box.artifact){const a=box.artifact;const mk=G.markers.find(m=>m.channel===a.channel&&a.size_bp>=m.min_bp&&a.size_bp<=m.max_bp);
  ctxMenu(e.clientX,e.clientY,[{label:T('renameArtifact')+' ('+a.label+')',run:()=>edit(box.sample,{action:'rename_artifact',artifactId:a.id,marker:a.marker,new:a.label})},
   {label:T('deleteArtifact')+' ('+a.label+')',run:()=>edit(box.sample,{action:'delete_artifact',artifactId:a.id,marker:a.marker})},
   {label:T('addAlleleLabel'),disabled:!mk,run:()=>edit(box.sample,{action:'artifact_to_allele',artifactId:a.id,marker:mk.name,new:a.orig??binLabel(mk,a.size_bp)})}]);return}
 if(box){const c=box.call;ctxMenu(e.clientX,e.clientY,[{label:T('renameAllele')+' ('+c.label+')',run:()=>edit(box.sample,{action:'rename',marker:c.marker,callId:c.id,new:c.label})},
   {label:T('toArtifact')+' ('+c.label+')',run:()=>edit(box.sample,{action:'to_artifact',marker:c.marker,callId:c.id,new:''})},
   {label:T('deleteAllele')+' ('+c.label+')',run:()=>edit(box.sample,{action:'delete',marker:c.marker,callId:c.id})}]);return}
 const mk=G.markers.find(m=>m._hit&&x>=m._hit.x0&&x<=m._hit.x1&&y>=m._hit.y0&&y<=m._hit.y1);
 if(mk){const row=(s.markers||[]).find(r=>r.marker===mk.name);
  ctxMenu(e.clientX,e.clientY,[{label:T('overrideGq')+' ('+mk.name+')',run:()=>edit(s,{action:'override_gq',marker:mk.name})},
   {label:(row?.pli?'✓ ':'')+T('pliMenu'),run:()=>edit(s,{action:'pli',marker:mk.name,new:row?.pli?'off':'on'})}]);return}
 const near=nearPeak(G,x);
 if(near){const calls=near.sample.report?.calls||[];const arts=near.sample.report?.artifacts||[];const bp=near.peak.size_bp;
  const isLabeled=(smp,tag,b)=>(smp.report?.calls||[]).some(c=>c.channel===tag&&Math.abs(c.size_bp-b)<0.05)||(smp.report?.artifacts||[]).some(a=>a.channel===tag&&Math.abs(a.size_bp-b)<0.05);
  const called=isLabeled(near.sample,near.tag,bp);
  const marker=G.markers.find(m=>m.channel===near.tag&&bp>=m.min_bp&&bp<=m.max_bp);const peak={scan:near.peak.scan,size_bp:bp,height:near.peak.height,channel:near.tag};
  const sel=[...view.selPeaks.values()].filter(p=>!isLabeled(p.sample,p.tag,p.peak.size_bp));const inSel=view.selPeaks.has(peakKey(near.sample,near.tag,near.peak));
  const items=[{label:T('addAlleleLabel')+` (${bp.toFixed(2)} bp)`,disabled:called||!marker,
   run:()=>edit(near.sample,{action:'add',marker:marker.name,new:binLabel(marker,bp),peak})}];
  if(inSel&&sel.length>1)items.push({label:fmt('addArtifactMany',sel.length),run:()=>artifactMany(sel)});
  else items.push({label:T('addArtifactLabel')+` (${bp.toFixed(2)} bp)`,disabled:called,run:()=>edit(near.sample,{action:'add_artifact',marker:marker?.name||null,new:'',peak})});
  items.push('-',{label:T('peakRawData'),run:()=>{try{window.opener.OT.peakRawData(near.sample.id,near.peak.scan);window.opener.focus()}catch(e){message(String(e.message||e))}}});
  ctxMenu(e.clientX,e.clientY,items)}});
// One artifact label and one reason for several selected peaks.
async function artifactMany(list){if(window.Sec&&!Sec.can('UPDATE_PROJECT'))return Sec.denied();
 const label=await artifactDialog(T('act_add_artifact'),'');if(label==null)return;
 const specs=list.map(p=>{const bp=p.peak.size_bp;const cv=[...document.querySelectorAll('#plotarea canvas')].find(c=>c._geom&&c._geom.sample.id===p.sample.id);
  const mk=(cv?._geom.markers||[]).find(m=>m.channel===p.tag&&bp>=m.min_bp&&bp<=m.max_bp);
  return {sample:p.sample,spec:{action:'add_artifact',marker:mk?.name||null,new:label,peak:{scan:p.peak.scan,size_bp:bp,height:p.peak.height,channel:p.tag}}}});
 let items=[];try{for(const x of specs)items.push(...window.opener.OT.editItems(x.sample.id,x.spec))}catch(e){}
 const ch=window.Sec?await Sec.change({items,title:T('reasonTitle')}):{reason:''};if(!ch)return;
 for(const x of specs){try{await window.opener.OT.edit(x.sample.id,{...x.spec,reason:ch.reason})}catch(e){message(String(e.message||e));break}}
 view.selPeaks.clear();try{payload=window.opener.OT.plotPayload()}catch(e){}build();draw()}
// Multiple Dye Plot: all dyes of a sample within the range of the selected (or first) marker
function multipleDyePlot(){
 const cv=[...document.querySelectorAll('#plotarea canvas')].find(c=>c._geom&&(view.selSample==null||c._geom.sample.id===view.selSample))||document.querySelector('#plotarea canvas');
 const G=cv?._geom;if(!G)return message(T('selectSamplesFirst'));const s=G.sample;
 const m=G.markers.find(x=>x.name===view.selMarker)||(isGeno()?G.markers[0]:null);if(!m)return message(T('selectMarkerHeader'));
 const tags=(s.analysis?.channels||[]).filter(c=>c.group!=='auxiliary'&&DYE_KEYS.includes(c.tag)&&c.tag!==s.analysis?.sizing?.reference_channel).map(c=>c.tag);
 dialog(fmt('multipleDyeTitle',m.name,s.name),b=>{b.classList.add('mdpdlg');const c=el('canvas',{style:'width:900px;max-width:86vw'});c.height=320;b.append(c);
  const mg=setting?.display?.markerMargin??5,cur=curveOf(s.analysis);let a=m.min_bp-mg,z=m.max_bp+mg;if(view.xAxis==='dp'){a=bpToScan(cur,a);z=bpToScan(cur,z)}
  requestAnimationFrame(()=>paint(c,[{sample:s,dyes:tags}],a,z,m))},[[T('ok'),()=>true,true]])}
function zoom(f){const [d0,d1]=domain();const a=view.x0??d0,b=view.x1??d1,c=(a+b)/2,w=(b-a)*f/2;view.x0=c-w;view.x1=c+w;view.userZoom=true;draw()}

// ---------- lower: genotypes table + QVD ----------
function renderLower(){
 $('lower').style.display=view.table==='none'?'none':'flex';
 const grid=$('gtGrid');grid.replaceChildren();if(!payload)return;
 const t=el('table',{class:'g'});
 const SC=(setting?.sizingTable?.cols)||{};const scols=PlotSettings.SIZING_COLS.filter(([id])=>SC[id]!==false);
 if(view.table==='sizing'&&setting){t.style.font=`${setting.sizingTable.size}px ${setting.sizingTable.font}`}
 const cols=view.table==='sizing'?[T('colSampleName'),...scols.map(([id,,h])=>T('psCol_'+id)==='psCol_'+id?h:T('psCol_'+id))]:[T('colSampleName'),T('colMarker'),T('colAllele')+' 1',T('colAllele')+' 2','GQ'];
 const hr=el('tr');for(const c of cols)hr.append(el('th',{text:c}));t.append(el('thead',{},hr));const tb=el('tbody');
 for(const s of payload.samples){
  if(view.table==='sizing'){const letters={DATA1:'B',DATA2:'G',DATA3:'Y',DATA4:'R',DATA105:'P',DATA106:'O'};const nth={};
   for(const c of s.report?.calls||[]){const pk=(s.report.peaks||[]).find(p=>p.id===c.source_peak_id);nth[c.channel]=(nth[c.channel]||0)+1;
    const val={sampleFileName:s.file,marker:c.marker,dyeSamplePeak:`${letters[c.channel]||'?'},${nth[c.channel]}`,minutes:'',size:c.size_bp.toFixed(2),
     height:String(Math.round(c.height)),area:String(pk?.area??''),dataPoint:String(Math.round(c.scan)),alleleCall:String(c.label)};
    tb.append(el('tr',{},el('td',{text:s.name}),...scols.map(([id])=>el('td',{class:['size','height','area','dataPoint'].includes(id)?'num':'',text:val[id]}))))}continue}
  const gsel=isGeno()?new Set(payload.genoSel||[]):null;
  for(const m of (s.markers||[]).filter(m=>!gsel||gsel.has(s.id+'|'+m.marker))){const tr=el('tr',{class:view.selMarker===m.marker&&view.selSample===s.id?'sel':''});
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
 document.title=`${T('app')} : ${T(isGeno()?'genotypesPlot':'samplesPlot')}`;
 const set=(k,v)=>()=>{view[k]=v;build();draw()};
 const ck=(k,v)=>view[k]===v;
 const dyeItems=DYE_KEYS.map(t=>({label:T('dyes')+': '+DYE_NAMES[t],checked:view.dyes.has(t),run:()=>{view.dyes.has(t)?view.dyes.delete(t):view.dyes.add(t);build();draw()}}));
 buildMenubar($('menubar'),[
  {label:T('file'),items:[{label:T('exportTable'),sc:'Ctrl+E',run:exportPlotTable},'-',{label:T('pageSetup'),disabled:true},{label:T('printPreview'),disabled:true},{label:T('print'),sc:'Ctrl+P',run:()=>window.print()},'-',{label:T('closePlot'),run:()=>window.close()}]},
  {label:T('edit'),items:[{label:T('undo'),sc:'Ctrl+Z',disabled:true},{label:T('copy'),sc:'Ctrl+C',disabled:true}]},
  {label:T('view'),items:[{label:T('sampleInfo'),disabled:true},{label:T('rawData'),disabled:true},{label:T('eptData'),disabled:true},'-',
   {label:T('language'),sub:[{label:'Русский',checked:I18N.lang==='ru',run:()=>{I18N.set('ru');build();draw()}},{label:'English',checked:I18N.lang==='en',run:()=>{I18N.set('en');build();draw()}}]}]},
  {label:T('plots'),items:[{label:T('multipleDyePlot'),run:multipleDyePlot},'-',{label:T('zoom'),sub:[{label:T('zoomIn'),sc:'Ctrl+=',run:()=>zoom(.5)},{label:T('zoomOut'),sc:'Ctrl+-',run:()=>zoom(2)},{label:T('fullView'),run:()=>{view.x0=view.x1=null;draw()}}]},
   {label:T('xAxis'),sub:[{label:T('basepairs'),checked:view.xAxis==='bp',run:()=>{view.xAxis='bp';view.x0=view.x1=null;build();draw()}},{label:T('dataPoints'),checked:view.xAxis==='dp',run:()=>{view.xAxis='dp';view.x0=view.x1=null;build();draw()}}]},
   {label:T('yAxis'),sub:[{label:T('scaleInd'),checked:view.yAxis==='individual',run:()=>{view.yAxis='individual';build();draw()}},{label:T('scaleMaxY'),checked:view.yAxis==='max',run:()=>{view.yAxis='max';build();draw()}},
    {label:T('psScaleEntered')+` (${view.yValue})`,checked:view.yAxis==='entered',run:()=>{view.yAxis='entered';build();draw()}}]},
   {label:T('psPlotColors'),sub:[{label:T('psStandardColors'),checked:!view.customColors,run:()=>{view.customColors=false;build();draw()}},{label:T('psCustom'),checked:view.customColors,run:()=>{view.customColors=true;build();draw()}}]},
   {label:T('psPeakPositions'),checked:view.peakPositions,run:()=>{view.peakPositions=!view.peakPositions;build();draw()}},
   {label:T('labels'),sub:[{label:T('noLabels'),checked:ck('labels','none'),run:set('labels','none')},{label:T('hLabels'),checked:ck('labels','h'),run:set('labels','h')},{label:T('vLabels'),checked:ck('labels','v'),run:set('labels','v')}]},
   {label:T('dyes'),sub:dyeItems},'-',
   {label:T('combine'),checked:ck('layout','combine'),run:set('layout','combine')},{label:T('separate'),checked:ck('layout','separate'),run:set('layout','separate')},{label:T('overlay'),checked:ck('layout','overlay'),run:set('layout','overlay')},'-',
   {label:T('showHeader'),checked:view.header,run:set('header',!view.header)},{label:T('markerRange'),checked:view.markerRange,run:set('markerRange',!view.markerRange)},{label:T('showBins'),checked:view.bins,run:set('bins',!view.bins)},'-',
   {label:T('table'),sub:[{label:T('noTable'),checked:ck('table','none'),run:set('table','none')},{label:T('sizingTable'),checked:ck('table','sizing'),run:set('table','sizing')},{label:T('genotypesTable'),checked:ck('table','genotypes'),run:set('table','genotypes')}]}]},
  {label:T('tools'),items:[{label:T('manager'),disabled:true}]},
  {label:T('alleles'),items:[{label:T('editHowTo'),run:()=>message(T('editHint'))},{label:T('deletePredefined')+'...',run:deletePredefined},'-',{label:T('history'),run:()=>{try{window.opener.OT.labelEditViewer()}catch(e){}}}]},
  {label:T('help'),items:[{label:T('about'),run:()=>message(T('aboutText'),T('about'))}]}]);
 const t1=$('toolbar1');t1.replaceChildren(tbutton('print',T('print'),()=>window.print()),tbutton('printPreview',T('printPreview'),null,{disabled:true}),tsep(),el('span',{class:'tlabel',text:T('plotSetting')}));
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
 const H=(isGeno()?setting?.genotypeHeader:setting?.sampleHeader)||{};const on=k=>H[k]!==false;
 const ch=$('colhead');if(isGeno()){ch.replaceChildren(...(on('sampleFileName')?[el('div',{class:'c',style:'width:250px',text:T('colSampleFile')})]:[]),
  ...(on('sampleName')?[el('div',{class:'c',style:'width:150px',text:T('colSampleName')})]:[]),...(on('panel')?[el('div',{class:'c',style:'width:150px',text:T('colPanel')})]:[]),
  ...(on('marker')?[el('div',{class:'c',style:'width:100px',text:T('colMarker')})]:[]),...GENO_PQV.filter(on).map(k=>el('div',{class:'c',style:'width:46px',text:k})));ch.style.display=view.header?'flex':'none'}
 else ch.replaceChildren(...(on('sampleFileName')?[el('div',{class:'c',style:'width:250px',text:T('colSampleFile')})]:[]),
  ...(on('sampleName')?[el('div',{class:'c',style:'width:150px',text:T('colSampleName')})]:[]),...(on('panel')?[el('div',{class:'c',style:'width:150px',text:T('colPanel')})]:[]),
  ...PQV_KEYS.filter(on).map(k=>el('div',{class:'c',style:'width:46px',text:k})));if(!isGeno())ch.style.display=view.header?'flex':'none';
}
document.addEventListener('keydown',e=>{if(!(e.ctrlKey||e.metaKey))return;if(e.key.toLowerCase()==='e'){e.preventDefault();exportPlotTable();return}if(e.key==='='||e.key==='+'){e.preventDefault();zoom(.5)}if(e.key==='-'){e.preventDefault();zoom(2)}});
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
