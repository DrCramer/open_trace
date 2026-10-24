// Info, Raw Data and EPT Data tabs of the Project window (ID-X View > Sample Info / Raw Data / EPT Data).
(function(){
const $=id=>document.getElementById(id);
const DYE_COLORS={1:'#1f4fd6',2:'#1a9b2e',3:'#222',4:'#d32020',5:'#e67e22',6:'#7a35b5'};
const EPT_COLORS={voltage:'#d32020',current:'#1a9b2e',power:'#1f4fd6',temperature:'#e67e22'};
let ctx=null;const cache=new WeakMap();
const view={raw:{x0:null,x1:null,hidden:new Set()}};

function load(s){if(!s.blob)return Promise.reject(Error(T('siNoFile')));
 if(!cache.has(s))cache.set(s,(async()=>{const r=await fetch('/api/sample-info?'+new URLSearchParams({filename:s.file}),{method:'POST',
  headers:{'X-Session-Token':window.OT_TOKEN,'X-User-Ticket':window.Sec?Sec.ticket():''},body:s.blob});const j=await r.json();if(!r.ok)throw Error(j.error||'Error');return j})());
 const p=cache.get(s);p.catch(()=>cache.delete(s));return p}
const label=key=>{const k='si_'+key,t=T(k);return t===k?key:t};
function kv(rows){const t=el('table',{class:'g sitable'});const tb=el('tbody');for(const [k,v] of rows)tb.append(el('tr',{},el('td',{class:'k',text:k}),el('td',{text:v==null||v===''?'—':String(v)})));t.append(tb);return t}

async function renderInfo(page,list){
 page.replaceChildren();if(!list.length){page.append(el('div',{class:'empty',text:T('siSelect')}));return}
 const wrap=el('div',{class:'siwrap'});page.append(wrap);
 for(const s of list.slice(0,8)){const box=el('div',{class:'sibox','data-file':s.file});wrap.append(box);
  const x=ctx.extra(s);
  box.append(el('h3',{text:`${s.file} — ${s.name}`}),
   el('div',{class:'legend',text:T('siSampleFileInfo')}),kv([[label('sampleFile'),s.file],[label('sampleName'),s.name],[label('sampleOrigin'),`${s.runFolder}/${s.file}`],[label('statusMessage'),x.status],[label('sampleType'),x.sampleType]]),
   el('div',{class:'legend',text:T('siErrors')}),kv(x.errors.length?x.errors.map((e,i)=>[String(i+1),e]):[[T('siNone'),'']]),
   el('div',{class:'legend',text:T('siLastSettings')}),kv(x.settings));
  const slot=el('div',{class:'sidyn',text:T('siLoading')});box.append(slot);
  try{const r=await load(s);slot.replaceChildren(...r.sections.map(sec=>[el('div',{class:'legend',text:T('siSec_'+sec.id)}),kv(sec.items.map(i=>[label(i.key),i.value]))]).flat())}
  catch(e){slot.replaceChildren(el('div',{class:'note',text:e.message}))}}
 if(list.length>8)wrap.append(el('div',{class:'note',text:T('siFirst8')}))}

// ---------- plots ----------
function axes(g,W,H,L,B,x0,x1,ymin,ymax,xlab){g.strokeStyle='#888';g.fillStyle='#333';g.font='10px Tahoma,sans-serif';g.lineWidth=1;
 g.beginPath();g.moveTo(L,4);g.lineTo(L,H-B);g.lineTo(W-6,H-B);g.stroke();
 const nice=(span,n)=>{const r=span/n,p=10**Math.floor(Math.log10(r||1)),m=r/p;return (m<1.5?1:m<3?2:m<7?5:10)*p};
 const sx=nice(x1-x0,8);for(let v=Math.ceil(x0/sx)*sx;v<=x1;v+=sx){const x=L+(v-x0)/(x1-x0)*(W-6-L);g.fillText(String(Math.round(v*100)/100),x-8,H-B+12);g.beginPath();g.moveTo(x,H-B);g.lineTo(x,H-B+3);g.stroke()}
 const sy=nice(ymax-ymin||1,4);for(let v=Math.ceil(ymin/sy)*sy;v<=ymax;v+=sy){const y=H-B-(v-ymin)/((ymax-ymin)||1)*(H-B-8);g.fillText(String(Math.round(v*100)/100),2,y+3);g.strokeStyle='#eee';g.beginPath();g.moveTo(L+1,y);g.lineTo(W-6,y);g.stroke();g.strokeStyle='#888'}
 if(xlab)g.fillText(xlab,W-6-g.measureText(xlab).width,H-2)}
function trace(g,vals,x0,x1,ymin,ymax,W,H,L,B,color){const n=vals.length;if(!n)return;g.strokeStyle=color;g.beginPath();const pw=W-6-L;
 const i0=Math.max(0,Math.floor(x0)),i1=Math.min(n-1,Math.ceil(x1));const per=(i1-i0)/pw;
 const Y=v=>H-B-(v-ymin)/((ymax-ymin)||1)*(H-B-8);
 if(per<=2){for(let i=i0;i<=i1;i++){const x=L+(i-x0)/(x1-x0)*pw;i===i0?g.moveTo(x,Y(vals[i])):g.lineTo(x,Y(vals[i]))}}
 else for(let px=0;px<=pw;px++){const a=Math.floor(i0+px*per),b=Math.min(i1,Math.floor(i0+(px+1)*per));let lo=Infinity,hi=-Infinity;for(let i=a;i<=b;i++){const v=vals[i];if(v<lo)lo=v;if(v>hi)hi=v}
  if(lo===Infinity)continue;const x=L+px;if(px===0)g.moveTo(x,Y(lo));else g.lineTo(x,Y(lo));g.lineTo(x,Y(hi))}
 g.stroke()}
function makeCanvas(holder,h){const c=el('canvas',{class:'sicanvas'});holder.append(c);const W=Math.max(400,holder.clientWidth-4),H=h;c.width=W*devicePixelRatio;c.height=H*devicePixelRatio;
 c.style.width=W+'px';c.style.height=H+'px';const g=c.getContext('2d');g.scale(devicePixelRatio,devicePixelRatio);return {c,g,W,H}}

async function renderRaw(page,list){
 page.replaceChildren();const s=list[0];if(!s){page.append(el('div',{class:'empty',text:T('siSelect')}));return}
 const head=el('div',{class:'sihead'},el('b',{text:`${s.file} — ${s.name}`}));page.append(head);const holder=el('div',{class:'siplot'});page.append(holder);
 const read=el('div',{class:'sistatus'});page.append(read);
 let r;try{holder.textContent=T('siLoading');r=await load(s)}catch(e){holder.replaceChildren(el('div',{class:'note',text:e.message}));return}
 holder.textContent='';const v=view.raw;
 const legend=el('span',{class:'sileg'});for(const ch of r.raw){const cb=el('input',{type:'checkbox','data-dye':String(ch.dye_index)});cb.checked=!v.hidden.has(ch.dye_index);
  cb.addEventListener('change',()=>{cb.checked?v.hidden.delete(ch.dye_index):v.hidden.add(ch.dye_index);draw()});
  legend.append(el('label',{class:'chk',style:`color:${DYE_COLORS[ch.dye_index]||'#000'}`},cb,document.createTextNode(` ${ch.tag} ${ch.dye||''}`)))}
 const reset=el('button',{class:'btn',type:'button',text:T('siFullView')});reset.addEventListener('click',()=>{v.x0=v.x1=null;draw()});
 head.append(legend,reset);
 const L=46,B=18;let G;
 const draw=()=>{holder.replaceChildren();G=makeCanvas(holder,Math.max(260,page.clientHeight-80));const {g,W,H}=G;
  const n=r.scan_count||r.raw[0]?.values.length||1;const x0=v.x0??0,x1=v.x1??n-1;
  let ymax=1,ymin=0;for(const ch of r.raw){if(v.hidden.has(ch.dye_index))continue;for(let i=Math.max(0,Math.floor(x0));i<=Math.min(ch.values.length-1,Math.ceil(x1));i++){const y=ch.values[i];if(y>ymax)ymax=y;if(y<ymin)ymin=y}}
  ymax*=1.05;G.x0=x0;G.x1=x1;G.ymin=ymin;G.ymax=ymax;
  axes(g,W,H,L,B,x0,x1,ymin,ymax,T('siScan'));
  for(const ch of r.raw)if(!v.hidden.has(ch.dye_index))trace(g,ch.values,x0,x1,ymin,ymax,W,H,L,B,DYE_COLORS[ch.dye_index]||'#000');
  let drag=null;const cv=G.c;const inv=px=>G.x0+(px-L)/(W-6-L)*(G.x1-G.x0);
  cv.addEventListener('mousedown',e=>{drag=e.offsetX});
  cv.addEventListener('mouseup',e=>{if(drag==null)return;const a=inv(Math.min(drag,e.offsetX)),b=inv(Math.max(drag,e.offsetX));drag=null;if(b-a>5){v.x0=Math.max(0,a);v.x1=b;draw()}});
  cv.addEventListener('dblclick',()=>{v.x0=v.x1=null;draw()});
  cv.addEventListener('mousemove',e=>{const x=Math.round(inv(e.offsetX));const vals=r.raw.filter(c=>!v.hidden.has(c.dye_index)).map(c=>`${c.dye||c.tag}: ${c.values[x]??''}`);read.textContent=`${T('siScan')} ${x}   ${vals.join('   ')}`})};
 draw()}

async function renderEpt(page,list){
 page.replaceChildren();const s=list[0];if(!s){page.append(el('div',{class:'empty',text:T('siSelect')}));return}
 page.append(el('div',{class:'sihead'},el('b',{text:`${s.file} — ${s.name}`})));const holder=el('div',{class:'siept'});page.append(holder);
 let r;try{holder.textContent=T('siLoading');r=await load(s)}catch(e){holder.replaceChildren(el('div',{class:'note',text:e.message}));return}
 holder.textContent='';const keys=Object.keys(r.ept);if(!keys.length){holder.append(el('div',{class:'note',text:T('siNoEpt')}));return}
 const dur=(r.sections.find(x=>x.id==='run')?.items.find(i=>i.key==='runDuration')?.value)||null;
 const minutes=dur?dur.split(':').reduce((a,b)=>a*60+Number(b),0)/60:null;
 for(const k of keys){const e=r.ept[k];const row=el('div',{class:'eptrow'});holder.append(row);
  row.append(el('div',{class:'eptlab',style:`color:${EPT_COLORS[k]}`,text:`${T('siEpt_'+k)} (${e.tag}) · ${T('siMin')} ${e.min} · ${T('siMax')} ${e.max}`}));
  const h=el('div');row.append(h);const {g,W,H}=makeCanvas(h,Math.max(90,Math.floor((page.clientHeight-60)/keys.length)-26));
  const n=e.values.length,L=46,B=16;const span=minutes?minutes:n-1;
  axes(g,W,H,L,B,0,span,Math.min(0,e.min),e.max*1.1||1,minutes?T('siMinutes'):T('siPoint'));
  trace(g,e.values,0,n-1,Math.min(0,e.min),e.max*1.1||1,W,H,L,B,EPT_COLORS[k])}
 holder.append(el('div',{class:'note',text:T('siEptNote')}))}

function render(tab){const page=$('page-'+tab);if(!page||!ctx)return;const list=ctx.samples();
 const key=tab+':'+list.map(s=>s.id).join(',');if(page.dataset.key===key&&page.childElementCount)return;page.dataset.key=key;
 ({info:renderInfo,raw:renderRaw,ept:renderEpt})[tab](page,list)}
window.addEventListener('resize',()=>{for(const t of ['raw','ept']){const p=$('page-'+t);if(p&&!p.hidden){p.dataset.key='';render(t)}}});
window.SampleViews={init(c){ctx=c},render,invalidate(){for(const t of ['info','raw','ept']){const p=$('page-'+t);if(p)p.dataset.key=''}}};
})();
