// Plot settings: ID-X Plot Settings Editor (General, Sample Header, Genotype Header, Sizing Table, Labels,
// Display Settings tabs), the GeneMapper ID-X Manager "Plot Settings" tab and PlotSettingContainer XML.
// Field names follow the Castor descriptors of GMp.jar (XmlPlotSetting, PlotDisplaySetting).
(function(){
const KEY='opentrace.plotSettings.v1';
const DYES=['blue','green','yellow','red','purple','orange'];
const clone=x=>JSON.parse(JSON.stringify(x));
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
// [id, XmlPlotSetting field, header]
const SAMPLE_HEADER=[['sampleFileName','sdrSampleFileNameSelected','Sample File'],['sampleName','sdrSampleNameSelected','Sample Name'],
 ['panel','sdrPanelSelected','Panel'],['SOS','sdrPqvOSselected','SOS'],['SQ','sdrPqvSQselected','SQ'],['SSPK','sdrSpikeSamplePqvQualitySelected','SSPK'],
 ['MIX','sdrMixedSamplePqvQualitySelected','MIX'],['OMR','sdrOmrSamplePqvQualitySelected','OMR'],['CGQ','sdrSampleGenotypeQualitySelected','CGQ']];
const GENO_HEADER=[['sampleFileName','gdrSampleFileNameSelected','Sample File'],['sampleName','gdrSampleNameSelected','Sample Name'],
 ['panel','gdrPanelSelected','Panel'],['marker','gdrMarkerSelected','Marker'],['AN','gdrPqvANEselected','AN'],['BIN','gdrPqvBINselected','BIN'],
 ['PHR','gdrPqvPHRselected','PHR'],['LPH','gdrPqvLPHselected','LPH'],['MPH','gdrPqvHPHselected','MPH'],['BD','gdrPqvBDselected','BD'],
 ['SPK','gdrSpikeMarkerPqvSelected','SPK'],['OS','gdrPqvOSselected','OS'],['OVL','gdrPqvOverLapSelected','OVL'],['ACC','gdrPqvACCselected','ACC'],
 ['CC','gdrPqvCCselected','CC'],['GQ','gdrPqvGQselected','GQ']];
const SIZING_COLS=[['sampleFileName','stSampleFileNameSelected','Sample File'],['marker','stMarkerSelected','Marker'],
 ['dyeSamplePeak','dyeSamplePeakSelected','Dye/Sample Peak'],['minutes','minutesSelected','Minutes'],['size','sizeSelected','Size'],
 ['height','peakHeightSelected','Height'],['area','peakAreaSelected','Area'],['dataPoint','dataPointSelected','Data Point'],['alleleCall','alleleCallSelected','Allele Call']];
// label content per peak type (Labels tab; up to four lines); prefixes as in the help topic
const LABEL_TYPES=[['none','None',''],['alleleCall','Allele Call','al'],['size','Size','sz'],['height','Height','ht'],['area','Area','ar'],
 ['dataPoint','Data Point','dp'],['aeReason','AE Reason for Change','ae']];
const LABEL_KINDS=[['assigned','assignedAlleleLabel','Assigned Allele'],['custom','customAlleleLabel','Custom Allele'],
 ['ladder','allelicLadderLabel','Allelic Ladder'],['artifact','artifactLabel','Artifact']];
const FONTS=['Arial','Tahoma','Verdana','Times New Roman','Courier New'];
const all=(rows,v)=>Object.fromEntries(rows.map(r=>[r[0],v]));

function base(name,description){return {name,description:description||'',
 sampleHeader:all(SAMPLE_HEADER,true),genotypeHeader:all(GENO_HEADER,true),
 sizingTable:{cols:all(SIZING_COLS,true),font:'Arial',size:11},
 labels:{assigned:['alleleCall','none','none','none'],custom:['alleleCall','none','none','none'],ladder:['alleleCall','none','none','none'],
  artifact:['alleleCall','none','none','none'],font:'Tahoma',size:11,color:'bw',prefixes:false,editType:false,pqvTrigger:false,virtualBlack:false},
 display:{persistence:'these',panes:2,header:true,markerRange:true,markerIndicator:true,bins:true,peakPositions:false,alleleChanges:true,toolbar:true,offscale:true,
  controlsTop:false,laddersTop:false,sizeStdLabels:false,labels:'h',xAxis:'bp',yAxis:'individual',yValue:1000,
  dyes:Object.fromEntries(DYES.map(d=>[d,true])),allDyes:true,start:0,end:500,table:'genotypes',layout:'separate',printQVD:false,
  customColors:['#1f4fd6','#1a9b2e','#b59a00','#d32020','#7a35b5','#e67e22','#000000','#00a3a3'],markerMargin:5}}}
// Factory settings: "Data Interpretation" (glossary: two panes, 75-460 bp, Genotypes table and QVD, blue/green/yellow/red)
// and a sizing review setting (Sizing table, all dyes, size-standard labels).
function factory(){
 const di=base('Data Interpretation','Default setting to evaluate anomalies using quality value flags and details');
 Object.assign(di.display,{panes:2,allDyes:false,start:75,end:460,table:'genotypes',dyes:{blue:true,green:true,yellow:true,red:true,purple:false,orange:false}});
 const sz=base('Sizing','Size standard and sizing review');
 Object.assign(sz.display,{panes:1,allDyes:true,table:'sizing',layout:'combine',sizeStdLabels:true});sz.labels.assigned=['alleleCall','size','none','none'];
 return [di,sz]}

function validate(s){
 if(!s||typeof s!=='object')throw Error(T('xmlInvalid'));
 s.name=String(s.name||'').trim();if(!s.name)throw Error(T('psNameRequired'));if(s.name.length>255)throw Error(T('psNameLong'));
 if(/[<>"\\]/.test(s.name))throw Error(T('psNameChars'));
 const d=base(s.name);
 const merge=(dst,src)=>{for(const k of Object.keys(dst)){if(!src||!(k in src))continue;const v=src[k];
  if(dst[k]&&typeof dst[k]==='object'&&!Array.isArray(dst[k]))merge(dst[k],v);else if(Array.isArray(dst[k])&&Array.isArray(v))dst[k]=v.slice(0,dst[k].length).concat(dst[k].slice(v.length));
  else if(typeof dst[k]===typeof v)dst[k]=v}};
 merge(d,s);d.name=s.name;
 const D=d.display;D.panes=Math.max(1,Math.min(16,Math.round(D.panes)||1));
 for(const k of ['start','end'])if(!(D[k]>=0&&D[k]<=10000))throw Error(T('psRangeNumbers'));
 if(!D.allDyes&&D.end<=D.start)throw Error(T('psEndGtStart'));
 if(!(D.yValue>0))D.yValue=1000;D.markerMargin=Math.max(0,Math.round(D.markerMargin)||0);
 for(const k of ['size'])if(!(d.labels[k]>=1&&d.labels[k]<=20))throw Error(T('psFontSize'));
 if(!(d.sizingTable.size>=1&&d.sizingTable.size<=20))throw Error(T('psFontSize'));
 for(const [k] of LABEL_KINDS){const l=d.labels[k];if(!l.every(x=>LABEL_TYPES.some(t=>t[0]===x)))throw Error(T('xmlInvalid'))}
 if(!['h','v','none'].includes(D.labels))D.labels='h';if(!['bp','dp'].includes(D.xAxis))D.xAxis='bp';
 if(!['individual','max','entered'].includes(D.yAxis))D.yAxis='individual';if(!['none','sizing','genotypes','lev'].includes(D.table))D.table='genotypes';
 if(!['combine','separate','overlay'].includes(D.layout))D.layout='separate';if(!['bw','dyeAll','dyeBorder'].includes(d.labels.color))d.labels.color='bw';
 if(!['these','last'].includes(D.persistence))D.persistence='these';
 return d}

function load(){try{const v=JSON.parse(localStorage.getItem(KEY)||'null');if(v&&Array.isArray(v.settings)&&v.settings.length)return v}catch(e){}
 return {settings:factory().map(s=>({setting:s,saved:new Date().toISOString()}))}}
function store(db){try{localStorage.setItem(KEY,JSON.stringify(db));return true}catch(e){return false}}
function list(){return load().settings}
function get(name){return list().find(r=>r.setting.name===name)?.setting||null}
function put(s,previous){s=validate(s);const db=load();const i=db.settings.findIndex(r=>r.setting.name===(previous??s.name));
 const row={setting:s,saved:new Date().toISOString()};if(i>=0)db.settings[i]=row;else db.settings.push(row);
 if(!store(db))throw Error(T('storageFull'));window.dispatchEvent(new Event('opentrace-plotsettings'));return s}
function remove(name){const db=load();db.settings=db.settings.filter(r=>r.setting.name!==name);store(db);window.dispatchEvent(new Event('opentrace-plotsettings'))}

// ---------- PlotSettingContainer XML ----------
function esc(x){return String(x).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;')}
const DYE_XML={blue:'Blue',green:'Green',yellow:'Yellow',red:'Red',purple:'Purple',orange:'Orange'};
// enumerations: positions in the ID-X editor lists (1-based), as PlotDisplaySetting keeps them
const E={persistence:['last','these'],labels:['none','h','v'],xAxis:['dp','bp'],yAxis:['individual','max','entered'],
 table:['none','sizing','genotypes','lev'],layout:['combine','separate','overlay'],color:['bw','dyeAll','dyeBorder']};
const enc=(k,v)=>E[k].indexOf(v)+1,dec=(k,n,d)=>E[k][(parseInt(n,10)||0)-1]??d;
function toXml(s){
 const D=s.display,L=s.labels;const b=(n,v)=>`<${n}>${!!v}</${n}>`;const t=(n,v)=>`<${n}>${esc(v)}</${n}>`;
 let x='<?xml version="1.0" encoding="UTF-8"?>\n<PlotSettingContainer xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="PlotSettingContainer">'+
  '<appVersion>1.6</appVersion><fileVersion>1.0</fileVersion><xmlPlotSetting>'+t('name',s.name)+t('comments',s.description)+t('userName','OpenTrace');
 for(const [id,f] of SAMPLE_HEADER)x+=b(f,s.sampleHeader[id]);for(const [id,f] of GENO_HEADER)x+=b(f,s.genotypeHeader[id]);
 for(const [id,f] of SIZING_COLS)x+=b(f,s.sizingTable.cols[id]);x+=t('sizingTableFontName',s.sizingTable.font)+t('sizingTableFontSize',s.sizingTable.size);
 for(const [k,f] of LABEL_KINDS)x+=t(f,L[k].join(','));
 x+=t('labelingFontName',L.font)+t('labelingFontSize',L.size)+t('labelingColorScheme',enc('color',L.color))+b('showDataTypePrefix',L.prefixes)+
  b('showManualChangePrefix',L.editType)+b('showPqvTriggerPeak',L.pqvTrigger)+b('invertMutantLabel',L.virtualBlack);
 x+='<plotDisplaySetting>'+t('persistenceMode',enc('persistence',D.persistence))+t('numPane',D.panes)+b('showPaneHeader',D.header)+b('showMarkerRange',D.markerRange)+
  b('showMarkerIndicator',D.markerIndicator)+b('showBin',D.bins)+b('showPeakPositionIndicators',D.peakPositions)+b('showAlleleEditHistory',D.alleleChanges)+
  b('showToolBar',D.toolbar)+b('showOffscale',D.offscale)+b('controlsAtTopOfPane',D.controlsTop)+b('laddersAtTopOfPane',D.laddersTop)+b('showSizeLabels',D.sizeStdLabels)+
  t('labelStatus',enc('labels',D.labels))+t('xAxisMode',enc('xAxis',D.xAxis))+t('yAxisScaleMode',enc('yAxis',D.yAxis))+t('yAxisScaleToUseValue',D.yValue)+
  b('dyeAllSelected',D.allDyes)+DYES.map(d=>b('dye'+DYE_XML[d]+'Selected',D.dyes[d])).join('')+t('allDyeStartRange',D.start)+t('allDyeEndRange',D.end)+
  t('tableMode',enc('table',D.table))+t('dyeMode',enc('layout',D.layout))+b('printQualityValueDetails',D.printQVD)+t('customColors',D.customColors.join(','))+
  t('markerMargin',D.markerMargin)+'</plotDisplaySetting></xmlPlotSetting></PlotSettingContainer>\n';
 return x}
function fromXml(text){
 if(text.length>1048576)throw Error('XML > 1 MiB');if(/<!DOCTYPE|<!ENTITY/i.test(text))throw Error('DTD');
 const doc=new DOMParser().parseFromString(text,'application/xml');if(doc.getElementsByTagName('parsererror').length)throw Error(T('xmlInvalid'));
 const p=doc.getElementsByTagName('xmlPlotSetting')[0];if(!p)throw Error(T('psNoSetting'));
 const child=(n,name)=>n&&[...n.children].find(c=>c.localName===name);const txt=(n,name)=>child(n,name)?.textContent.trim();
 const bool=(n,name,d)=>{const v=txt(n,name);return v==null?d:v==='true'};
 const s=base(txt(p,'name')||'Imported',txt(p,'comments')||'');
 for(const [id,f] of SAMPLE_HEADER)s.sampleHeader[id]=bool(p,f,true);for(const [id,f] of GENO_HEADER)s.genotypeHeader[id]=bool(p,f,true);
 for(const [id,f] of SIZING_COLS)s.sizingTable.cols[id]=bool(p,f,true);
 s.sizingTable.font=txt(p,'sizingTableFontName')||s.sizingTable.font;s.sizingTable.size=+(txt(p,'sizingTableFontSize')||s.sizingTable.size);
 if(!(s.sizingTable.size>=1&&s.sizingTable.size<=20))throw Error(T('psImportSizingFont'));
 for(const [k,f] of LABEL_KINDS){const v=txt(p,f);if(v){const a=v.split(',').map(x=>x.trim()).filter(x=>LABEL_TYPES.some(t=>t[0]===x));s.labels[k]=a.concat(['none','none','none','none']).slice(0,4)}}
 const L=s.labels;L.font=txt(p,'labelingFontName')||L.font;L.size=+(txt(p,'labelingFontSize')||L.size);if(!(L.size>=1&&L.size<=20))throw Error(T('psImportLabelFont'));
 L.color=dec('color',txt(p,'labelingColorScheme'),'bw');L.prefixes=bool(p,'showDataTypePrefix',false);L.editType=bool(p,'showManualChangePrefix',false);
 L.pqvTrigger=bool(p,'showPqvTriggerPeak',false);L.virtualBlack=bool(p,'invertMutantLabel',false);
 const d=child(p,'plotDisplaySetting');const D=s.display;
 if(d){D.persistence=dec('persistence',txt(d,'persistenceMode'),'these');D.panes=+(txt(d,'numPane')||D.panes);
  for(const [k,f] of [['header','showPaneHeader'],['markerRange','showMarkerRange'],['markerIndicator','showMarkerIndicator'],['bins','showBin'],['peakPositions','showPeakPositionIndicators'],
   ['alleleChanges','showAlleleEditHistory'],['toolbar','showToolBar'],['offscale','showOffscale'],['controlsTop','controlsAtTopOfPane'],['laddersTop','laddersAtTopOfPane'],
   ['sizeStdLabels','showSizeLabels'],['allDyes','dyeAllSelected'],['printQVD','printQualityValueDetails']])D[k]=bool(d,f,D[k]);
  D.labels=dec('labels',txt(d,'labelStatus'),'h');D.xAxis=dec('xAxis',txt(d,'xAxisMode'),'bp');D.yAxis=dec('yAxis',txt(d,'yAxisScaleMode'),'individual');
  D.yValue=+(txt(d,'yAxisScaleToUseValue')||D.yValue);for(const dy of DYES)D.dyes[dy]=bool(d,'dye'+DYE_XML[dy]+'Selected',D.dyes[dy]);
  D.start=+(txt(d,'allDyeStartRange')||D.start);D.end=+(txt(d,'allDyeEndRange')||D.end);D.table=dec('table',txt(d,'tableMode'),'genotypes');
  D.layout=dec('layout',txt(d,'dyeMode'),'separate');const cc=txt(d,'customColors');if(cc)D.customColors=cc.split(',').map(x=>x.trim()).filter(x=>/^#[0-9a-f]{6}$/i.test(x)).concat(D.customColors).slice(0,8);
  D.markerMargin=+(txt(d,'markerMargin')||D.markerMargin)}
 return validate(s)}

// ---------- editor ----------
function openEditor(source,{title,onSave,restrict}={}){
 const s=clone(source);const previous=source.name;let tab='general';const ro=restrict==='read';
 const d=dialog(title||T('psEditor'),b=>{b.classList.add('pseditdlg')},
  [...(ro?[]:[[T('ok'),()=>{try{s.name=(s.name||'').trim();if(!s.name){message(T('psNewName'));return false}
    if(s.name!==previous&&get(s.name)){message(T('psNameExists'));return false}const v=validate(s);onSave&&onSave(v,previous);return true}catch(e){message(e.message);return false}},true]]),
   [T(ro?'close':'cancel'),()=>true]]);
 const check=(obj,key,label,attrs={})=>{const c=el('input',{type:'checkbox',...attrs});c.checked=!!obj[key];c.disabled=ro;c.addEventListener('change',()=>{obj[key]=c.checked});
  return el('label',{class:'chk'},c,document.createTextNode(' '+label))};
 const radio=(obj,key,value,label,name)=>{const r=el('input',{type:'radio',name,value,'data-ps':key+'='+value});r.checked=obj[key]===value;r.disabled=ro;r.addEventListener('change',()=>{if(r.checked){obj[key]=value;draw()}});
  return el('label',{class:'chk'},r,document.createTextNode(' '+label))};
 const num=(obj,key,attrs={})=>{const i=el('input',{type:'text',class:'num',value:String(obj[key]),...attrs});i.disabled=ro;i.addEventListener('change',()=>{obj[key]=Number(i.value)});return i};
 const select=(obj,key,opts,attrs={})=>{const x=el('select',attrs);for(const [v,l] of opts)x.append(new Option(l,v));x.value=String(obj[key]);x.disabled=ro;
  x.addEventListener('change',()=>{obj[key]=typeof obj[key]==='number'?Number(x.value):x.value});return x};
 const field=(label,input)=>el('div',{class:'field'},el('label',{text:label}),input);
 const checks=(rows,obj,legend,attr)=>{const fs=el('fieldset',{},el('legend',{text:legend}));const grid=el('div',{class:'psgrid'});
  for(const [id,,head] of rows)grid.append(check(obj,id,head,{[attr]:id}));
  const bar=el('div',{class:'btnrow'});for(const [lbl,v] of [[T('psShowAll'),true],[T('psHideAll'),false]]){const bt=el('button',{class:'btn',type:'button',text:lbl});bt.disabled=ro;
   bt.addEventListener('click',()=>{for(const [id] of rows)obj[id]=v;draw()});bar.append(bt)}
  fs.append(grid,bar);return fs};
 const TABS=[['general','psGeneral'],['sample','psSampleHeader'],['geno','psGenotypeHeader'],['sizing','psSizingTable'],['labels','psLabels'],['display','psDisplay']];
 const draw=()=>{const b=d.body;b.replaceChildren();
  const tabs=el('div',{class:'tabs dtabs'});for(const [id,k] of TABS){const t=el('div',{class:'tab'+(tab===id?' on':''),text:T(k),'data-tab':id});t.addEventListener('mousedown',()=>{tab=id;draw()});tabs.append(t)}
  const page=el('div',{class:'tabpage dpage'});const D=s.display,L=s.labels;
  if(tab==='general'){const n=el('input',{type:'text',id:'psName',value:s.name,maxlength:'255'});n.disabled=ro;n.addEventListener('input',()=>{s.name=n.value});
   const ds=el('input',{type:'text',id:'psDesc',value:s.description});ds.disabled=ro;ds.addEventListener('input',()=>{s.description=ds.value});
   page.append(field(T('amName'),n),field(T('psSecurityNote'),el('span',{text:window.Sec&&Sec.items?Sec.items.get('plots',previous).group:'GeneMapper ID-X Security Group'})),field(T('scDescription'),ds))}
  else if(tab==='sample')page.append(checks(SAMPLE_HEADER,s.sampleHeader,T('psSampleHeaderSettings'),'data-sh'));
  else if(tab==='geno')page.append(checks(GENO_HEADER,s.genotypeHeader,T('psGenotypeHeaderSettings'),'data-gh'),el('div',{class:'note',text:T('psGenoNote')}));
  else if(tab==='sizing'){page.append(checks(SIZING_COLS,s.sizingTable.cols,T('psColumnSettings'),'data-st'),
    el('fieldset',{},el('legend',{text:T('psFontSettings')}),field(T('psFont'),select(s.sizingTable,'font',FONTS.map(f=>[f,f]))),field(T('psSize'),num(s.sizingTable,'size',{id:'psStSize'}))))}
  else if(tab==='labels'){const t=el('table',{class:'g pslabels'});t.append(el('thead',{},el('tr',{},el('th',{text:''}),...[1,2,3,4].map(i=>el('th',{text:fmt('psLabelN',i)})))));
   const tb=el('tbody');for(const [k,,head] of LABEL_KINDS){const tr=el('tr',{},el('td',{text:T('psKind_'+k)}));
    L[k].forEach((v,i)=>{const x=el('select',{'data-label':k+i});for(const [id,lbl] of LABEL_TYPES)x.append(new Option(T('psLt_'+id)===('psLt_'+id)?lbl:T('psLt_'+id),id));x.value=v;x.disabled=ro;
     x.addEventListener('change',()=>{L[k][i]=x.value});tr.append(el('td',{},x))});tb.append(tr)}
   t.append(tb);
   page.append(el('fieldset',{},el('legend',{text:T('psShowLabels')}),t,el('div',{class:'note',text:T('psNoneNote')})),
    el('fieldset',{},el('legend',{text:T('psFontSettings')}),field(T('psFont'),select(L,'font',FONTS.map(f=>[f,f]))),field(T('psSize'),num(L,'size',{id:'psLabelSize'}))),
    check(L,'pqvTrigger',T('psPqvTrigger')),check(L,'prefixes',T('psPrefixes')),check(L,'editType',T('psEditType')),check(L,'virtualBlack',T('psVirtualBlack')),
    field(T('psLabelColor'),select(L,'color',[['bw',T('psColorBw')],['dyeAll',T('psColorDyeAll')],['dyeBorder',T('psColorDyeBorder')]],{id:'psColor'})))}
  else{
   const sh=el('fieldset',{},el('legend',{text:T('psShowHide')}));
   for(const [k,lbl] of [['header','showHeader'],['markerRange','markerRange'],['markerIndicator','psMarkerIndicators'],['bins','showBins'],['peakPositions','psPeakPositions'],
    ['alleleChanges','psAlleleChanges'],['toolbar','psToolbar'],['offscale','psOffscale'],['controlsTop','psControlsTop'],['laddersTop','psLaddersTop']])sh.append(check(D,k,T(lbl),{'data-show':k}));
   const lab=el('fieldset',{},el('legend',{text:T('labels')}),check(D,'sizeStdLabels',T('psSizeStdLabels')),
    radio(D,'labels','none',T('noLabels'),'psl'),radio(D,'labels','h',T('hLabels'),'psl'),radio(D,'labels','v',T('vLabels'),'psl'));
   const ax=el('fieldset',{},el('legend',{text:T('psAxes')}),el('div',{class:'inl'},el('b',{text:'X: '}),radio(D,'xAxis','dp',T('psDataPoints'),'psx'),radio(D,'xAxis','bp',T('psBasepairs'),'psx')),
    el('div',{class:'inl'},el('b',{text:'Y: '}),radio(D,'yAxis','individual',T('psScaleIndividually'),'psy'),radio(D,'yAxis','max',T('psScaleMax'),'psy'),radio(D,'yAxis','entered',T('psScaleEntered'),'psy'),num(D,'yValue',{id:'psYValue'})));
   const dyes=el('div',{class:'inl'});for(const dy of DYES)dyes.append(check(D.dyes,dy,T('dye_'+dy),{'data-dye':dy}));
   const sp=el('fieldset',{},el('legend',{text:T('psSamplePlotOnly')}),el('div',{class:'field'},el('label',{text:T('psSelectDyes')}),dyes),
    check(D,'allDyes',T('psAllDyes'),{id:'psAllDyes'}),el('div',{class:'inl'},el('span',{text:T('psStartRange')+' '}),num(D,'start',{id:'psStart'}),el('span',{text:' '+T('psEndRange')+' '}),num(D,'end',{id:'psEnd'}),el('span',{text:' '+T('bp')})),
    field(T('table'),select(D,'table',[['none',T('noTable')],['sizing',T('sizingTable')],['genotypes',T('genotypesTable')],['lev',T('labelEditViewer')]],{id:'psTable'})),
    field(T('psDyeLayout'),select(D,'layout',[['combine',T('combine')],['separate',T('separate')],['overlay',T('overlay')]],{id:'psLayout'})),
    check(D,'printQVD',T('psPrintQvd')),
    (()=>{const bt=el('button',{class:'btn',type:'button',text:T('psCustomColors')+'...'});bt.disabled=ro;bt.addEventListener('click',()=>customColors(D));return bt})());
   page.append(el('div',{class:'field'},el('label',{text:T('psWhenOpening')}),el('span',{},radio(D,'persistence','last',T('psUseLast'),'psp'),radio(D,'persistence','these',T('psUseThese'),'psp'))),
    field(T('panes'),num(D,'panes',{id:'psPanes'})),el('div',{class:'pscols'},el('div',{},sh,lab),el('div',{},ax,sp,
    el('fieldset',{},el('legend',{text:T('psGenotypePlotOnly')}),el('div',{class:'inl'},el('span',{text:T('psMarkerMargin')+' '}),num(D,'markerMargin',{id:'psMargin'}),el('span',{text:' '+T('bp')}))))),
    el('div',{class:'note',text:T('psRetainNote')}))}
  b.append(tabs,page)};
 draw();return d}
function customColors(D){
 const cols=D.customColors.slice();
 dialog(T('psDefineColors'),b=>{b.append(el('p',{text:T('psSelectColors')}));const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},el('th',{text:T('psTraceNo')}),el('th',{text:T('psColor')}))));
  const tb=el('tbody');cols.forEach((c,i)=>{const inp=el('input',{type:'color',value:c,'data-trace':String(i+1)});inp.addEventListener('input',()=>{cols[i]=inp.value});
   tb.append(el('tr',{},el('td',{class:'num',text:String(i+1)}),el('td',{},inp)))});t.append(tb);b.append(t)},
  [[T('ok'),()=>{D.customColors=cols;return true},true],[T('cancel'),()=>true]])}

if(window.Methods)Methods.registerKind(()=>({id:'plots',label:T('mgrPlots'),
 rows:()=>list().map(r=>({name:r.setting.name,saved:r.saved,a:r.setting.description||'',b:''})),
 heads:[T('scDescription'),''],get,put,remove,count:()=>list().length,
 create:async()=>{const f=base('');let n=1;while(get(`${T('psUntitled')} ${n}`))n++;f.name=`${T('psUntitled')} ${n}`;return f},
 edit:(obj,opts)=>openEditor(obj,opts),newTitle:T('psCreateNew'),
 importText:async text=>({obj:/^\s*[{]/.test(text)?validate(JSON.parse(text)):fromXml(text),note:''}),
 exportText:async obj=>toXml(obj),imported:T('psImported'),confirmDelete:'psConfirmDelete'}));

window.PlotSettings={KEY,DYES,SAMPLE_HEADER,GENO_HEADER,SIZING_COLS,LABEL_TYPES,LABEL_KINDS,list,get,put,remove,validate,factory,toXml,fromXml,openEditor,base};
})();
