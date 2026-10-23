// Project Options (File > Project Options): General, Add Samples and Analysis tabs as in ID-X 1.6.
// Settings are per browser profile, like the original's per-user options.
(function(){
const KEY='opentrace.projectOptions.v1';
const DEFAULTS={
 startup:'new',            // 'new' | 'last' project opened at start
 updatePlots:true,         // refresh an open plot window when the table selection changes
 printComment:false,comment:'',
 add:{method:'',sizeStd:'',panelKey:'',type:'auto'},
 ars:'stop',               // 'stop' | 'continue'  (Analysis Requirements Summary)
 alas:'stop',              // 'stop' | 'passing' | 'all'  (Allelic Ladder Analysis Summary)
 after:'summary',          // 'summary' | 'samples'
 lowQualityTop:false,
 metrics:'symbols',        // 'symbols' | 'numbers' for SQ and GQ
 dupHomo:false,            // show a single autosomal allele twice
};
const clone=x=>JSON.parse(JSON.stringify(x));
function load(){let v=null;try{v=JSON.parse(localStorage.getItem(KEY)||'null')}catch(e){}
 const o=clone(DEFAULTS);if(v&&typeof v==='object'){for(const k of Object.keys(DEFAULTS))if(k!=='add'&&typeof v[k]===typeof DEFAULTS[k])o[k]=v[k];
  if(v.add&&typeof v.add==='object')for(const k of Object.keys(DEFAULTS.add))if(typeof v.add[k]==='string')o.add[k]=v.add[k]}
 if(!['new','last'].includes(o.startup))o.startup='new';if(!['stop','continue'].includes(o.ars))o.ars='stop';
 if(!['stop','passing','all'].includes(o.alas))o.alas='stop';if(!['summary','samples'].includes(o.after))o.after='summary';
 if(!['symbols','numbers'].includes(o.metrics))o.metrics='symbols';o.comment=String(o.comment).slice(0,200);return o}
let cached=null;
function save(o){cached=o;try{localStorage.setItem(KEY,JSON.stringify(o))}catch(e){}window.dispatchEvent(new Event('opentrace-options'))}
function get(){return cached||(cached=load())}
window.addEventListener('storage',e=>{if(e.key===KEY)cached=null});

// ctx: {methodSelect(value,onChange), panelSelect(value,onChange), sizeStdNames(), types:[[value,label]...]}
function open(ctx){
 const o=clone(get());let tab='general';
 const radio=(name,value,label,cur,set)=>{const r=el('input',{type:'radio',name,value,'data-opt':name+'='+value});r.checked=cur===value;r.addEventListener('change',()=>{if(r.checked)set(value)});
  return el('label',{class:'chk'},r,document.createTextNode(' '+label))};
 const check=(id,label,cur,set)=>{const c=el('input',{type:'checkbox','data-opt':id});c.checked=!!cur;c.addEventListener('change',()=>set(c.checked));return el('label',{class:'chk'},c,document.createTextNode(' '+label))};
 const d=dialog(T('projectOptions').replace(/\.+$/,''),()=>{},[[T('ok'),()=>{save(o);return true},true],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();b.classList.add('optdlg');
  const tabs=el('div',{class:'tabs dtabs'});
  for(const [id,key] of [['general','optGeneral'],['add','optAddSamples'],['analysis','optAnalysis']]){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(key),'data-tab':id});t.addEventListener('mousedown',()=>{tab=id;draw()});tabs.append(t)}
  const page=el('div',{class:'tabpage dpage'});
  if(tab==='general'){
   page.append(el('fieldset',{},el('legend',{text:T('optStartup')}),
     radio('startup','new',T('optStartNew'),o.startup,v=>o.startup=v),radio('startup','last',T('optStartLast'),o.startup,v=>o.startup=v)),
    el('fieldset',{},el('legend',{text:T('optPlots')}),check('updatePlots',T('optUpdatePlots'),o.updatePlots,v=>o.updatePlots=v),
     check('printComment',T('optPrintComment'),o.printComment,v=>{o.printComment=v;cm.disabled=!v})),
    el('fieldset',{},el('legend',{text:T('optSecurity')}),el('select',{disabled:''},new Option('GeneMapper ID-X Security Group'))));
   const cm=el('input',{type:'text',id:'optComment',maxlength:'200',value:o.comment});cm.disabled=!o.printComment;cm.addEventListener('input',()=>{o.comment=cm.value});
   page.children[1].append(el('div',{class:'field'},el('label',{text:T('optComment')}),cm))}
  else if(tab==='add'){
   const ms=ctx.methodSelect(o.add.method,v=>{o.add.method=v});ms.prepend(new Option(T('optNone'),''));ms.value=o.add.method;ms.id='optMethod';
   const ss=el('select',{id:'optStd'});ss.append(new Option(T('optNone'),''));for(const n of ctx.sizeStdNames())ss.append(new Option(n,n));ss.value=o.add.sizeStd;ss.addEventListener('change',()=>{o.add.sizeStd=ss.value});
   const ps=ctx.panelSelect(o.add.panelKey,v=>{o.add.panelKey=v});ps.id='optPanel';
   const ts=el('select',{id:'optType'});ts.append(new Option(T('optTypeAuto'),'auto'));for(const [v,l] of ctx.types)ts.append(new Option(l,v));ts.value=o.add.type;ts.addEventListener('change',()=>{o.add.type=ts.value});
   page.append(el('p',{class:'note',text:T('optAddNote')}),
    el('div',{class:'field'},el('label',{text:T('colAnalysisMethod')}),ms),el('div',{class:'field'},el('label',{text:T('colSizeStandard')}),ss),
    el('div',{class:'field'},el('label',{text:T('colPanel')}),ps),el('div',{class:'field'},el('label',{text:T('colSampleType')}),ts))}
  else{
   page.append(
    el('fieldset',{},el('legend',{text:T('optArs')}),radio('ars','stop',T('optArsStop'),o.ars,v=>o.ars=v),radio('ars','continue',T('optArsContinue'),o.ars,v=>o.ars=v)),
    el('fieldset',{},el('legend',{text:T('optAlas')}),radio('alas','stop',T('optAlasStop'),o.alas,v=>o.alas=v),radio('alas','passing',T('alasPassing'),o.alas,v=>o.alas=v),radio('alas','all',T('alasAll'),o.alas,v=>o.alas=v)),
    el('fieldset',{},el('legend',{text:T('optAfter')}),radio('after','summary',T('optAfterSummary'),o.after,v=>o.after=v),radio('after','samples',T('optAfterSamples'),o.after,v=>o.after=v)),
    el('fieldset',{},el('legend',{text:T('optAutomatic')}),check('lowQualityTop',T('lowQualityTop'),o.lowQualityTop,v=>o.lowQualityTop=v)),
    el('fieldset',{},el('legend',{text:T('optMetrics')}),radio('metrics','symbols',T('optSymbols'),o.metrics,v=>o.metrics=v),radio('metrics','numbers',T('optNumbers'),o.metrics,v=>o.metrics=v)),
    el('fieldset',{},el('legend',{text:T('optDupHomo')}),check('dupHomo',T('optDupHomoText'),o.dupHomo,v=>o.dupHomo=v)))}
  b.append(tabs,page)};
 draw();return d}
window.ProjectOptions={KEY,DEFAULTS,get,load,save,open};
})();
