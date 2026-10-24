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
 else if(data.table){const t=el('table',{class:'ptable'+(page.fit?' fit':'')});const [head,...rows]=data.table;
  t.append(el('thead',{},el('tr',{},...head.map(x=>el('th',{text:String(x)})))));const body=el('tbody');
  for(const r of rows)body.append(el('tr',{},...r.map(x=>el('td',{text:x==null?'':String(x)}))));t.append(body);paper.append(t);
  paper.append(el('div',{class:'pfoot',text:`${rows.length} ${T('pvRows')}`}))}}
build();
if(location.hash==='#print')setTimeout(()=>window.print(),300);
})();
