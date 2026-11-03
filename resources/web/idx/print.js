// Print Preview: renders the opener's table (Samples, Genotypes, Analysis Summary or a report) on a page of the chosen size.
(function(){
const $=id=>document.getElementById(id);
const SIZES={A4:[210,297],Letter:[216,279],Legal:[216,356]};
let data=null;
try{data=window.opener&&window.opener.OT&&window.opener.OT.printPayload()}catch(e){data=null}
if(data&&data.lang)I18N.set(data.lang);
document.documentElement.lang=I18N.lang;
function build(){
 const tb=$('ptoolbar');tb.replaceChildren();
 const btn=(text,fn,id)=>{const b=el('button',{class:'btn',type:'button',text,id});b.addEventListener('click',fn);return b};
 tb.append(btn(T('print'),()=>window.print(),'pvPrint'),btn(T('closeWin'),()=>window.close(),'pvClose'));
 const paper=$('paper');paper.replaceChildren();
 if(!data){paper.append(el('p',{text:T('printNothing')}));return}
 const page=data.page||{orientation:'landscape',fit:true,header:true,paper:'A4'};
 const [w,h]=SIZES[page.paper]||SIZES.A4;const [pw,ph]=page.orientation==='portrait'?[w,h]:[h,w];
 const st=el('style');st.textContent=`@page{size:${page.paper.toLowerCase()} ${page.orientation};margin:10mm}.paper{width:${pw-20}mm;min-height:${ph-20}mm}`;document.head.append(st);
 document.title=`${data.title} — ${data.project}`;
 if(page.header)paper.append(el('div',{class:'phead'},el('b',{text:data.project}),el('span',{text:data.title+(data.subtitle?' — '+data.subtitle:'')}),el('span',{text:data.date})));
 if(data.html!=null){const d=el('div',{class:'summary pbody'});d.innerHTML=data.html;d.querySelectorAll('select').forEach(s=>s.disabled=true);paper.append(d)}
 else if(data.table){
  const [head,...rows]=data.table;const fontPx=page.screenFont===false?(page.fontSize||10)*96/72:null;
  const cell=(tag,x)=>el(tag,{text:x==null?'':String(x)});
  // 'Start New Page': a page break whenever the sample changes (column named like Sample Name, else the first)
  const sc=Math.max(0,head.findIndex(h=>/sample name|имя образца/i.test(String(h))));
  const mk=(cols,rs,brk)=>{const t=el('table',{class:'ptable'+(page.fit?' fit':'')+(brk?' brk':'')});if(fontPx)t.style.setProperty('--pfs',fontPx+'px');
   t.append(el('thead',{},el('tr',{},...cols.map(i=>cell('th',head[i])))));const body=el('tbody');
   let prev=null;for(const r of rs){const tr=el('tr',{},...cols.map(i=>cell('td',r[i])));if(page.newPage&&prev!==null&&String(r[sc])!==prev)tr.className='newpage';prev=String(r[sc]);body.append(tr)}t.append(body);return t};
  const all=head.map((_,i)=>i);
  if(page.fit)paper.append(mk(all,rows));
  else{ // not fitted: columns are split into page-wide groups; 'column first' prints one column group down all rows, 'row first' a block of rows across all groups
   const charMm=0.19*(page.screenFont===false?(page.fontSize||10):10),usable=pw-20;
   const wmm=i=>Math.min(60,Math.max(String(head[i]).length,...rows.slice(0,60).map(r=>String(r[i]??'').length)))*charMm+3;
   const groups=[];let cur=[],w=0;for(const i of all){const x=wmm(i);if(cur.length&&w+x>usable){groups.push(cur);cur=[];w=0}cur.push(i);w+=x}if(cur.length)groups.push(cur);
   const rowsPerPage=Math.max(5,Math.floor((ph-30)/(charMm*3.6+1)));
   if(page.order==='row'){for(let a=0;a<rows.length;a+=rowsPerPage)for(const g of groups)paper.append(mk(g,rows.slice(a,a+rowsPerPage),true))}
   else for(const g of groups)paper.append(mk(g,rows,true))}
  paper.append(el('div',{class:'pfoot',text:`${rows.length} ${T('pvRows')}`}))}}
build();
if(location.hash==='#print')setTimeout(()=>window.print(),300);
})();
