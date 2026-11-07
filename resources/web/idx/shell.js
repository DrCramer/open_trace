// OpenTrace desktop shell: command registry, user interface layout (visibility and order of menus, menu items,
// toolbar buttons, workspace tabs and panels), presets, themes and the plot surface, command palette and the
// "Customize interface" editor. It is a UI adapter over the existing handlers: commands keep their own code, a hidden
// button or menu item only removes one way to call it (shortcuts, the palette and other places keep working).
//
// Stored per user and computer (desktop: /desktop/api/ui-layout, atomic file next to desktop.json; web: localStorage),
// schema `desktop_ui_layout_v1`. Nothing here touches analysis methods, panels, standards or project data.
(function(){
'use strict';
const SCHEMA=1;
const KEY='opentrace.uiLayout.v1';
const DESKTOP=!!window.OT_DESKTOP||location.protocol==='opentrace:'||location.hostname==='opentrace.localhost';
const WIN=document.documentElement.dataset.window||'main';

// ---------- strings ----------
I18N.add('en',{
 shStatusEdit:'Editing',shStatusRead:'Read only',shStatusLocal:'Local',shStatusNoProject:'No project',shStatusDirty:'unsaved changes',
 shCommands:'Commands',shCommandsTip:'Find a command (Ctrl+Shift+P)',shTheme:'Theme',shThemeSystem:'System',shThemeLight:'Light',shThemeGraphite:'Graphite',
 shThemeDark:'Dark',shThemeContrast:'High contrast',shQuick:'Interface settings',shToggleTheme:'Light / dark',shUser:'User',shNoUser:'no sign-in',
 shCustomize:'Customize interface...',shReset:'Restore interface',shResetConfirm:'Restore the factory interface (menus, buttons, tabs, panels, theme)? Project data and laboratory settings are not affected.',
 shPalette:'Command palette...',shPreset:'Workspace layout',shDensity:'Density',shCompact:'Compact',shStandard:'Standard',shLabels:'Button captions',shLabelsOff:'Icons only',
 shLabelsOn:'Icons with captions',shPlotSurface:'Plot background',shSurfaceAuto:'Auto (by theme)',shSurfaceLight:'Light',shSurfaceDark:'Dark',shChrome:'Window style',
 shChromeIndustrial:'Industrial (new)',shChromeClassic:'Classic (ID-X like)',
 shPresetStandard:'Standard',shPresetAnalysis:'Analysis',shPresetManual:'Manual interpretation',shPresetQc:'Quality control',shPresetDual:'Two monitors',shPresetCustom:'Custom',
 shPresetCustomNone:'Custom (not saved yet)',
 shEdTitle:'Customize interface',shEdAreas:'Areas',shEdShow:'Show',shEdCommand:'Command / element',shEdPath:'Path',shEdKey:'Shortcut',shEdState:'State',shEdPos:'Position',
 shEdSearch:'Search by name or ID',shEdAll:'All',shEdShown:'Shown',shEdHidden:'Hidden',shEdUnavailable:'Not available now',shEdUp:'Up',shEdDown:'Down',
 shEdFactory:'Factory interface',shEdResetArea:'Reset this area',shEdExport:'Export...',shEdImport:'Import...',shEdSaveCustom:'Save as layout',shEdApply:'Apply',
 shEdCancel:'Cancel',shEdGeneral:'General',shEdNote:'Hiding removes one way to call a command; shortcuts and the command palette keep it. Changes apply only to the view: no re-analysis, project data and laboratory settings stay as they are.',
 shEdUnknown:'Unknown IDs in the stored layout (commands of another window or removed): {0}',shEdDropUnknown:'Remove unknown',shEdNotBuilt:'This window has not been opened in this session: its elements appear here after it is opened once.',
 shEdId:'ID',shEdDisabled:'disabled now',shEdCritical:'always shown',shEdImported:'Layout imported.',shEdBadFile:'Not an OpenTrace interface layout: {0}',
 shPaletteHint:'Type to search; Enter runs the command; Esc closes.',shPaletteNone:'No commands found',shPaletteDisabled:'not available now',
 shCorrupt:'The stored interface layout could not be read; the standard layout is used. ({0})',shWsProjects:'Projects',shWsPlots:'Plots',shWsGenotypes:'Genotypes',
 shWsStandards:'Standards',shWsJournal:'Journal',shMore:'More',shProfile:'Laboratory profile',shProfileNone:'no profile',shMenuArea:'Menu',shToolbarArea:'Toolbar',
 shTabsArea:'Workspace tabs',shPanelsArea:'Panels',shStatusArea:'Status bar',shMainWindow:'Main window',shPlotWindow:'Plot window',shPanelsWindow:'Panel Manager',
 shReportWindow:'Report Manager',shServer:'Server',shPlotsMenu:'Plots',dsStandards:'Laboratory standards...',dsConnection:'Organization server...',dsProvenance:'Analysis provenance...',shTopMenus:'Menu sections',shStatusText:'Status message',shProgress:'Progress',shCursorXY:'Cursor position (X / Y)',shMarkerInfo:'Marker / peak information',shPhrCell:'Peak height ratio (PHR)',shPlotSurfaceWin:'Plot background (this window)',shSurfaceCommon:'As in interface settings',shPlotToolbar1:'Toolbar: printing, settings, dyes',shPlotToolbar2:'Toolbar: plot view',shEditBarArea:'Allele editing bar',shRightGroup:'Right group',shResetDone:'Interface restored.',
});
I18N.add('ru',{
 shStatusEdit:'Редактирование',shStatusRead:'Только чтение',shStatusLocal:'Локально',shStatusNoProject:'Нет проекта',shStatusDirty:'есть несохранённые правки',
 shCommands:'Команды',shCommandsTip:'Поиск команды (Ctrl+Shift+P)',shTheme:'Тема',shThemeSystem:'Системная',shThemeLight:'Светлая',shThemeGraphite:'Графитовая',
 shThemeDark:'Тёмная',shThemeContrast:'Контрастная',shQuick:'Настройки интерфейса',shToggleTheme:'Светлая / тёмная',shUser:'Пользователь',shNoUser:'без входа',
 shCustomize:'Настроить интерфейс...',shReset:'Восстановить интерфейс',shResetConfirm:'Вернуть заводской интерфейс (меню, кнопки, вкладки, панели, тема)? Данные проектов и лабораторные настройки не затрагиваются.',
 shPalette:'Палитра команд...',shPreset:'Раскладка рабочего места',shDensity:'Плотность',shCompact:'Компактная',shStandard:'Стандартная',shLabels:'Подписи кнопок',shLabelsOff:'Только значки',
 shLabelsOn:'Значки с подписями',shPlotSurface:'Фон графиков',shSurfaceAuto:'Авто (по теме)',shSurfaceLight:'Светлый',shSurfaceDark:'Тёмный',shChrome:'Вид окон',
 shChromeIndustrial:'Промышленный (новый)',shChromeClassic:'Классический (как ID-X)',
 shPresetStandard:'Стандартная',shPresetAnalysis:'Анализ',shPresetManual:'Ручная интерпретация',shPresetQc:'Контроль качества',shPresetDual:'Два монитора',shPresetCustom:'Пользовательская',
 shPresetCustomNone:'Пользовательская (ещё не сохранена)',
 shEdTitle:'Настройка интерфейса',shEdAreas:'Области',shEdShow:'Показ',shEdCommand:'Команда / элемент',shEdPath:'Путь',shEdKey:'Клавиши',shEdState:'Состояние',shEdPos:'Позиция',
 shEdSearch:'Поиск по названию или ID',shEdAll:'Все',shEdShown:'Отображаемые',shEdHidden:'Скрытые',shEdUnavailable:'Недоступные сейчас',shEdUp:'Выше',shEdDown:'Ниже',
 shEdFactory:'Заводской интерфейс',shEdResetArea:'Сбросить эту область',shEdExport:'Экспорт...',shEdImport:'Импорт...',shEdSaveCustom:'Сохранить как раскладку',shEdApply:'Применить',
 shEdCancel:'Отмена',shEdGeneral:'Общие',shEdNote:'Скрытие убирает один способ вызова: горячие клавиши и палитра команд продолжают работать. Меняется только вид: без повторного анализа, данные проекта и лабораторные настройки не меняются.',
 shEdUnknown:'Неизвестные ID в сохранённой раскладке (команды другого окна или удалённые): {0}',shEdDropUnknown:'Удалить неизвестные',shEdNotBuilt:'Это окно ещё не открывалось в этом сеансе: его элементы появятся здесь после первого открытия.',
 shEdId:'ID',shEdDisabled:'сейчас недоступна',shEdCritical:'показывается всегда',shEdImported:'Раскладка загружена.',shEdBadFile:'Это не раскладка интерфейса OpenTrace: {0}',
 shPaletteHint:'Введите часть названия; Enter — выполнить; Esc — закрыть.',shPaletteNone:'Команды не найдены',shPaletteDisabled:'сейчас недоступна',
 shCorrupt:'Сохранённую раскладку интерфейса прочитать не удалось, открыта стандартная. ({0})',shWsProjects:'Проекты',shWsPlots:'Графики',shWsGenotypes:'Генотипы',
 shWsStandards:'Стандарты',shWsJournal:'Журнал',shMore:'Ещё',shProfile:'Лабораторный профиль',shProfileNone:'без профиля',shMenuArea:'Меню',shToolbarArea:'Панель инструментов',
 shTabsArea:'Вкладки рабочих областей',shPanelsArea:'Панели',shStatusArea:'Строка состояния',shMainWindow:'Главное окно',shPlotWindow:'Окно графиков',shPanelsWindow:'Менеджер панелей',
 shReportWindow:'Менеджер отчётов',shServer:'Сервер',shPlotsMenu:'Графики',dsStandards:'Лабораторные стандарты...',dsConnection:'Сервер организации...',dsProvenance:'Происхождение анализов...',shTopMenus:'Разделы меню',shStatusText:'Сообщение о состоянии',shProgress:'Ход выполнения',shCursorXY:'Положение курсора (X / Y)',shMarkerInfo:'Сведения о маркере / пике',shPhrCell:'Отношение высот пиков (PHR)',shPlotSurfaceWin:'Фон графиков (это окно)',shSurfaceCommon:'Как в настройках интерфейса',shPlotToolbar1:'Панель: печать, настройки, красители',shPlotToolbar2:'Панель: вид графика',shEditBarArea:'Панель правки аллелей',shRightGroup:'Правая группа',shResetDone:'Интерфейс восстановлен.',
});

// ---------- layout store ----------
const THEMES=['system','light','graphite','dark','contrast'];
// Desktop app: the new industrial shell; web client: the classic look stays the default (switchable in View).
const FACTORY={schema_version:SCHEMA,chrome:DESKTOP?'industrial':'classic',theme:DESKTOP?'graphite':'light',plot_surface:'auto',density:'compact',labels:'off',active_preset:'standard',
 hidden:{},order:{},custom:null,saved_at:null};
// Presets: visibility of UI elements and hints for the windows (applied once when the preset is chosen).
const PRESETS={
 standard:{hidden:{},hints:{main:{navigator:true},plot:{table:'genotypes',toolbar:true,panes:2}}},
 analysis:{hidden:{'plot.toolbar2':['noLabels','hLabels','vLabels']},hints:{main:{navigator:true},plot:{table:'none',toolbar:true,panes:5}}},
 manual:{hidden:{},hints:{main:{navigator:true},plot:{table:'genotypes',toolbar:true,panes:2}}},
 qc:{hidden:{},hints:{main:{navigator:true,lowQualityTop:true,tab:'summary'},plot:{table:'genotypes',toolbar:true,panes:2}}},
 dual:{hidden:{},hints:{main:{navigator:true},plot:{table:'none',toolbar:true,panes:5}}},
};
const clone=o=>JSON.parse(JSON.stringify(o));
let layout=clone(FACTORY);let loadProblem=null;
// Accepts any stored object; unknown fields are dropped, wrong types fall back to the factory value.
function validate(x){
 if(!x||typeof x!=='object'||Array.isArray(x))throw Error('not an object');
 const v=Number(x.schema_version);if(!(v>=0&&v<=SCHEMA))throw Error('schema_version '+x.schema_version);
 const out=clone(FACTORY);
 if(v<1){/* schema 0 (none released): nothing to convert */}
 const pick=(k,ok)=>{if(ok(x[k]))out[k]=x[k]};
 pick('chrome',a=>a==='industrial'||a==='classic');pick('theme',a=>THEMES.includes(a));pick('plot_surface',a=>['auto','light','dark'].includes(a));
 pick('density',a=>a==='compact'||a==='standard');pick('labels',a=>a==='off'||a==='on');pick('active_preset',a=>a in PRESETS||a==='custom');
 for(const k of ['hidden','order']){if(x[k]&&typeof x[k]==='object')for(const [area,ids] of Object.entries(x[k]))if(Array.isArray(ids))out[k][String(area)]=ids.filter(i=>typeof i==='string'&&i.length<200).slice(0,2000)}
 if(x.custom&&typeof x.custom==='object')out.custom={hidden:validate({schema_version:SCHEMA,hidden:x.custom.hidden||{}}).hidden,order:validate({schema_version:SCHEMA,order:x.custom.order||{}}).order};
 if(typeof x.saved_at==='string')out.saved_at=x.saved_at;
 return out}
function readLocal(){try{const t=localStorage.getItem(KEY);if(!t)return null;return validate(JSON.parse(t))}catch(e){loadProblem=String(e.message||e);return null}}
function writeLocal(){try{localStorage.setItem(KEY,JSON.stringify(layout))}catch(e){}}
layout=readLocal()||clone(FACTORY);
let chan=null;try{chan=new BroadcastChannel('opentrace-ui')}catch(e){}
async function persist(){layout.saved_at=new Date().toISOString();writeLocal();
 if(DESKTOP){try{await fetch('/desktop/api/ui-layout',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(layout)})}catch(e){}}
 try{chan&&chan.postMessage({type:'layout',layout})}catch(e){}}
async function loadDesktop(){if(!DESKTOP)return;try{const r=await fetch('/desktop/api/ui-layout');if(!r.ok)return;const j=await r.json();if(!j||!j.layout)return;
 let v;try{v=validate(j.layout)}catch(e){loadProblem=String(e.message||e);return}
 if(JSON.stringify(v)!==JSON.stringify(layout)){layout=v;writeLocal();changed('load')}}catch(e){}}

// ---------- registry ----------
const commands=new Map();      // id -> {id,label,path,sc,run,disabled,checked,area,kind,critical}
const areas=new Map();         // area -> {title,group,items:[id...] (factory order),kind}
function areaInfo(area,title,group,kind){let a=areas.get(area);if(!a){a={area,title,group,kind,items:[]};areas.set(area,a)}if(title)a.title=title;if(group)a.group=group;if(kind)a.kind=kind;return a}
const isHidden=(area,id)=>(layout.hidden[area]||[]).includes(id);
function ordered(area,list,idOf){const ord=layout.order[area];if(!ord||!ord.length)return list;
 const pos=new Map(ord.map((id,i)=>[id,i]));const keyed=list.map((x,i)=>({x,i,p:pos.has(idOf(x))?pos.get(idOf(x)):null}));
 // items without a stored position keep their factory place relative to the positioned ones
 const placed=keyed.filter(k=>k.p!=null).sort((a,b)=>a.p-b.p);const out=[];let pi=0;
 for(const k of keyed){if(k.p==null)out.push(k.x);else out.push(placed[pi++].x)}return out}
const slug=s=>String(s).toLowerCase().replace(/[^a-zа-яё0-9]+/gi,'-').replace(/^-|-$/g,'').slice(0,40)||'item';
const WIN_TITLES={main:'shMainWindow',plot:'shPlotWindow',panels:'shPanelsWindow',report:'shReportWindow'};

// Menus: buildMenubar() hands its spec here; returns the spec with the user's visibility/order and the shell items.
function prepareMenu(win,spec){
 const top=`${win}.menu`;areaInfo(top,T('shTopMenus'),win,'menu');
 const walk=(items,area,path)=>{const a=areaInfo(area,null,win,'menu');const seen=new Set();const out=[];
  for(const it of items){if(it==='-'){out.push('-');continue}
   let id=it.id||('~'+slug(it.label));if(seen.has(id))id+='~'+seen.size;seen.add(id);
   const full=`${area}/${id}`;if(!a.items.includes(id))a.items.push(id);
   commands.set(full,{id:full,local:id,area,label:String(it.label),path:path.concat(String(it.label)).join(' › '),sc:it.sc||'',run:it.run,disabled:!!it.disabled,checked:!!it.checked,
    kind:it.sub?'submenu':'action',dynamic:!it.id,window:win});
   let sub=it.sub;if(sub)sub=walk(sub,`${area}.${id}`,path.concat(String(it.label)));
   if(isHidden(area,id))continue;out.push(sub?{...it,sub}:it)}
  // collapse separators left by hidden items
  const clean=[];for(const x of ordered(area,out,x=>x==='-'?null:(x.id||'~'+slug(x.label))))if(x!=='-'||(clean.length&&clean[clean.length-1]!=='-'))clean.push(x);
  while(clean[clean.length-1]==='-')clean.pop();return clean};
 const tops=[];const seen=new Set();
 for(const t of spec){const id=t.id||slug(t.label);seen.add(id);const a=areaInfo(top);if(!a.items.includes(id))a.items.push(id);
  commands.set(`${top}/${id}`,{id:`${top}/${id}`,local:id,area:top,label:String(t.label),path:String(t.label),kind:'section',window:win});
  let items=walk(t.items,`${top}.${id}`,[String(t.label)]);
  if(id==='view')items=items.concat(['-',...shellItems(win)]);
  areaInfo(`${top}.${id}`,String(t.label),win,'menu');
  if(isHidden(top,id))continue;tops.push({...t,id,items})}
 return ordered(top,tops,t=>t.id)}
function shellItems(win){const a=`${win}.menu.view`;const its=[{id:'shell.customize',label:T('shCustomize'),run:()=>openEditor()},{id:'shell.palette',label:T('shPalette'),sc:'Ctrl+Shift+P',run:()=>openPalette()},
  {id:'shell.reset',label:T('shReset'),sc:'Ctrl+Shift+F12',run:()=>resetAll(true)}];
 const ai=areaInfo(a,null,win,'menu');
 for(const it of its){if(!ai.items.includes(it.id))ai.items.push(it.id);commands.set(`${a}/${it.id}`,{id:`${a}/${it.id}`,local:it.id,area:a,label:it.label,path:T('view')+' › '+it.label,sc:it.sc||'',run:it.run,kind:'action',window:win})}
 return its.filter(it=>!isHidden(a,it.id))}

// Toolbars, tab strips, status bars: elements with data-cmd inside a container with data-area; applied after every
// rebuild of the container (MutationObserver callbacks run before the next paint).
function applyContainer(el){const area=el.dataset.area;if(!area)return;
 const a=areaInfo(area,el.dataset.areaTitle?T(el.dataset.areaTitle):null,WIN,el.dataset.areaKind||'toolbar');
 const kids=[...el.children];
 for(const k of kids){const id=k.dataset.cmd;if(!id)continue;if(!a.items.includes(id))a.items.push(id);
  const label=(k.dataset.labelKey?T(k.dataset.labelKey):'')||k.dataset.label||k.title||k.getAttribute('aria-label')||k.textContent.trim()||id;
  // palette action: a button is clicked, a tab gets its mousedown; status cells and combo boxes are elements (no action)
  const target=k.matches('button')?k:k.matches('[role=tab]')?k:k.matches('select')||k.querySelector('select')?null:k.querySelector('button');
  const kind=k.dataset.kind||(target?'action':'element');
  commands.set(`${area}/${id}`,{id:`${area}/${id}`,local:id,area,label,path:(a.title||area)+' › '+label,sc:k.dataset.sc||'',
   run:kind!=='action'||!target?null:(()=>{if(target.disabled)return;if(target.matches('[role=tab]'))target.dispatchEvent(new MouseEvent('mousedown',{bubbles:true}));else target.click()}),
   disabled:!!(target&&target.disabled),kind,critical:!!k.dataset.critical,window:WIN});
  if(!k.dataset.label&&k.title)k.dataset.label=k.title;
  k.hidden=!k.dataset.critical&&isHidden(area,id)}
 // order: items keep the factory group (between separators) they were built in; after sorting by the stored order a
 // separator goes wherever two neighbours come from different factory groups.
 {let g=0;for(const k of kids){if(k.classList.contains('tsep'))g++;else if(k._shGrp==null)k._shGrp=g}}
 if((layout.order[area]||[]).length){const items=kids.filter(k=>!k.classList.contains('tsep'));const seps=kids.filter(k=>k.classList.contains('tsep'));
  const sorted=ordered(area,items,k=>k.dataset.cmd||null);const seq=[];let si=0;
  sorted.forEach((k,i)=>{if(i&&k._shGrp!==sorted[i-1]._shGrp){let sp=seps[si++];if(!sp){sp=document.createElement('span');sp.className='tsep'}seq.push(sp)}seq.push(k)});
  for(;si<seps.length;si++)seq.push(seps[si]);   // unused separators stay at the end (hidden below)
  if(!(seq.length===kids.length&&seq.every((x,i)=>x===kids[i])))mute(el,()=>el.append(...seq))}
 // separators with nothing visible on one side are hidden
 const vis=[...el.children];let last=null;for(const k of vis){if(k.classList.contains('tsep')){k.hidden=!last||last.classList.contains('tsep');if(!k.hidden)last=k}else if(!k.hidden)last=k}
 if(last&&last.classList.contains('tsep'))last.hidden=true}
// our own reordering must not trigger the observer again: its records are taken (dropped) right after the change, so
// a rebuild of the container by the window's code in the same task is still seen
const observers=new WeakMap();
function mute(el,fn){fn();const o=observers.get(el);if(o)o.takeRecords()}
function bind(el){if(!el||observers.has(el))return;const o=new MutationObserver(()=>applyContainer(el));observers.set(el,o);applyContainer(el);o.observe(el,{childList:true})}
function bindAll(root=document){root.querySelectorAll('[data-area]').forEach(bind)}

// ---------- themes ----------
const mq=window.matchMedia?matchMedia('(prefers-color-scheme: dark)'):null;
function resolvedTheme(){const t=layout.theme;return t==='system'?(mq&&mq.matches?'dark':'light'):t}
// Auto: dark plot field only with the dark theme (the graphite shell keeps the light, ID-X like field by default)
function plotSurface(){if(layout.plot_surface!=='auto')return layout.plot_surface;return layout.chrome==='industrial'&&resolvedTheme()==='dark'?'dark':'light'}
function applyTheme(){const h=document.documentElement;h.dataset.chrome=layout.chrome;h.dataset.theme=resolvedTheme();h.dataset.themeChoice=layout.theme;
 h.dataset.density=layout.density;h.dataset.labels=layout.labels;h.dataset.plotSurface=plotSurface();
 h.style.colorScheme=['dark','graphite'].includes(resolvedTheme())&&layout.chrome==='industrial'?'dark':'light'}
mq&&mq.addEventListener&&mq.addEventListener('change',()=>{if(layout.theme==='system')changed('theme')});

// ---------- change notification ----------
const listeners=[];
function changed(what){applyTheme();bindAll();document.querySelectorAll('[data-area]').forEach(applyContainer);renderRight();
 for(const f of listeners)try{f(what)}catch(e){console.error(e)}
 window.dispatchEvent(new CustomEvent('opentrace-ui',{detail:{what}}))}
chan&&(chan.onmessage=e=>{if(e.data&&e.data.type==='layout'){try{layout=validate(e.data.layout);writeLocal();changed('remote')}catch(err){}}});
window.addEventListener('storage',e=>{if(e.key===KEY&&e.newValue){try{layout=validate(JSON.parse(e.newValue));changed('remote')}catch(err){}}});

function set(k,v){layout[k]=v;persist();changed(k)}
function applyPreset(name){if(name==='custom'){if(!layout.custom)return;layout.hidden=clone(layout.custom.hidden||{});layout.order=clone(layout.custom.order||{})}
 else{const p=PRESETS[name];if(!p)return;layout.hidden=clone(p.hidden);layout.order={}}
 layout.active_preset=name;persist();changed('preset');
 const hints=name==='custom'?null:PRESETS[name].hints;window.dispatchEvent(new CustomEvent('opentrace-ui-preset',{detail:{name,hints:hints||{}}}));
 try{chan&&chan.postMessage({type:'preset',name,hints:hints||{}})}catch(e){}}
chan&&chan.addEventListener('message',e=>{if(e.data&&e.data.type==='preset')window.dispatchEvent(new CustomEvent('opentrace-ui-preset',{detail:{name:e.data.name,hints:e.data.hints}}))});
function resetAll(ask){const go=()=>{const keepCustom=layout.custom;layout=clone(FACTORY);layout.custom=keepCustom;persist();changed('reset');
  window.dispatchEvent(new CustomEvent('opentrace-ui-preset',{detail:{name:'standard',hints:PRESETS.standard.hints}}));toast(T('shResetDone'))};
 if(!ask)return go();dialog(T('shReset'),b=>b.append(el('p',{style:'max-width:440px',text:T('shResetConfirm')})),[[T('yes'),()=>{go();return true},true],[T('no'),()=>true]])}
function toast(text){const t=el('div',{class:'sh-toast',text});document.body.append(t);setTimeout(()=>t.remove(),2600)}

// ---------- right group of the application bar ----------
let statusProvider=null,rightHost=null;
const rightItems=()=>[
 {id:'status',critical:true},{id:'commands'},{id:'profile'},{id:'theme'},{id:'quick',critical:true},{id:'themeToggle'},{id:'user'}];
function renderRight(){if(!rightHost||layout.chrome!=='industrial'){if(rightHost)rightHost.hidden=layout.chrome!=='industrial';return}
 rightHost.hidden=false;const area=`${WIN}.right`;const a=areaInfo(area,T('shRightGroup'),WIN,'right');
 const make={
  status:()=>{const s=statusProvider?statusProvider():{kind:'local',text:T('shStatusLocal')};const e=el('span',{class:'sh-status sh-'+(s.kind||'local'),title:s.title||'',text:'● '+s.text});return e},
  commands:()=>{const b=el('button',{type:'button',class:'sh-btn sh-cmds',title:T('shCommandsTip')},el('span',{class:'sh-ico',text:'⌕'}),document.createTextNode(T('shCommands')),el('kbd',{text:'Ctrl+Shift+P'}));b.addEventListener('click',openPalette);return b},
  profile:()=>{const p=window.__otLabProfile;const e=el('span',{class:'sh-chip',title:T('shProfile'),text:p?(p.name+(p.version?' v'+p.version:'')+(p.verified?' ✓':' ⚠')):T('shProfileNone')});if(!p)e.classList.add('dim');return e},
  theme:()=>{const s=el('select',{class:'sh-select',title:T('shTheme'),'aria-label':T('shTheme')});for(const t of THEMES)s.append(new Option(T('shTheme'+t[0].toUpperCase()+t.slice(1)),t));s.value=layout.theme;
   s.addEventListener('change',()=>set('theme',s.value));return el('label',{class:'sh-lab'},el('span',{text:T('shTheme')}),s)},
  quick:()=>{const b=el('button',{type:'button',class:'sh-btn sh-ico-btn',title:T('shQuick'),text:'⚙'});b.addEventListener('click',e=>{e.stopPropagation();quickMenu(b)});return b},
  themeToggle:()=>{const b=el('button',{type:'button',class:'sh-btn sh-ico-btn',title:T('shToggleTheme'),text:'◐'});
   b.addEventListener('click',()=>{const r=resolvedTheme();set('theme',r==='dark'?'light':r==='light'?'graphite':'dark')});return b},
  user:()=>{const u=window.Sec&&Sec.user;const name=u?(u.name||u.username||u.login||''):'';const b=el('button',{type:'button',class:'sh-btn sh-user',title:T('shUser'),text:'◎ '+(name||T('shNoUser'))});
   b.addEventListener('click',e=>{e.stopPropagation();userMenu(b)});return b},
 };
 rightHost.replaceChildren();
 for(const it of ordered(area,rightItems(),x=>x.id)){if(!a.items.includes(it.id))a.items.push(it.id);
  const lbl={status:T('shStatusEdit')+' / '+T('shStatusRead'),commands:T('shCommands'),profile:T('shProfile'),theme:T('shTheme'),quick:T('shQuick'),themeToggle:T('shToggleTheme'),user:T('shUser')}[it.id];
  commands.set(`${area}/${it.id}`,{id:`${area}/${it.id}`,local:it.id,area,label:lbl,path:T('shRightGroup')+' › '+lbl,kind:'element',critical:!!it.critical,window:WIN,
   run:it.id==='commands'?openPalette:it.id==='quick'?()=>quickMenu(rightHost.querySelector('.sh-ico-btn')):null});
  if(it.id==='profile'&&!DESKTOP)continue;
  if(!it.critical&&isHidden(area,it.id))continue;rightHost.append(make[it.id]())}}
function popMenu(anchor,items){document.querySelectorAll('.menu.sh-pop').forEach(m=>m.remove());const m=buildMenu(items);m.classList.add('sh-pop','show');document.body.append(m);
 const r=anchor.getBoundingClientRect();m.style.top=r.bottom+'px';m.style.left=Math.max(0,Math.min(innerWidth-m.offsetWidth-4,r.right-m.offsetWidth))+'px';
 const close=e=>{if(!m.contains(e.target)){m.remove();document.removeEventListener('mousedown',close,true)}};setTimeout(()=>document.addEventListener('mousedown',close,true))}
function quickMenu(anchor){const L=layout;
 popMenu(anchor,[{label:T('shCustomize'),run:openEditor},{label:T('shPalette'),sc:'Ctrl+Shift+P',run:openPalette},'-',
  {label:T('shPreset'),sub:[...Object.keys(PRESETS).map(p=>({label:T('shPreset'+p[0].toUpperCase()+p.slice(1)),checked:L.active_preset===p,run:()=>applyPreset(p)})),
   {label:L.custom?T('shPresetCustom'):T('shPresetCustomNone'),checked:L.active_preset==='custom',disabled:!L.custom,run:()=>applyPreset('custom')}]},
  {label:T('shTheme'),sub:THEMES.map(t=>({label:T('shTheme'+t[0].toUpperCase()+t.slice(1)),checked:L.theme===t,run:()=>set('theme',t)}))},
  {label:T('shPlotSurface'),sub:[['auto','shSurfaceAuto'],['light','shSurfaceLight'],['dark','shSurfaceDark']].map(([v,k])=>({label:T(k),checked:L.plot_surface===v,run:()=>set('plot_surface',v)}))},
  {label:T('shDensity'),sub:[['compact','shCompact'],['standard','shStandard']].map(([v,k])=>({label:T(k),checked:L.density===v,run:()=>set('density',v)}))},
  {label:T('shLabels'),sub:[['off','shLabelsOff'],['on','shLabelsOn']].map(([v,k])=>({label:T(k),checked:L.labels===v,run:()=>set('labels',v)}))},
  {label:T('shChrome'),sub:[['industrial','shChromeIndustrial'],['classic','shChromeClassic']].map(([v,k])=>({label:T(k),checked:L.chrome===v,run:()=>set('chrome',v)}))},'-',
  {label:T('shReset'),sc:'Ctrl+Shift+F12',run:()=>resetAll(true)}])}
function userMenu(anchor){const S=window.Sec;const items=[];
 if(S){items.push({label:T('scChangePassword')+'...',disabled:!(S.user&&S.user.has_password),run:()=>S.changePassword()},{label:T('logOut'),disabled:!(S.state&&S.state.require_login),run:()=>S.logout()})}
 if(!items.length)items.push({label:T('shNoUser'),disabled:true});popMenu(anchor,items)}

// The industrial chrome moves the menu bar into one application bar with the right group pinned to the right edge.
function mountAppbar(){const bar=document.getElementById('menubar');if(!bar||document.getElementById('shAppbar'))return;
 const app=el('div',{class:'sh-appbar',id:'shAppbar'});bar.parentNode.insertBefore(app,bar);app.append(bar);
 rightHost=el('div',{class:'sh-right',id:'shRight'});app.append(rightHost);renderRight();
 // the menu sections keep their width; when the window is narrow the menu bar scrolls under the pinned right group
}

// ---------- command palette ----------
function allCommands(){return [...commands.values()].filter(c=>c.run&&c.kind!=='section'&&c.kind!=='submenu')}
function openPalette(){if(document.getElementById('shPalette'))return;
 const back=el('div',{class:'sh-palback',id:'shPalette'});const box=el('div',{class:'sh-palette',role:'dialog','aria-label':T('shCommands')});
 const inp=el('input',{type:'text',class:'sh-palin',placeholder:T('shPaletteHint')});const list=el('div',{class:'sh-pallist',role:'listbox'});
 box.append(inp,list);back.append(box);document.body.append(back);let sel=0,shown=[];
 const draw=()=>{const q=inp.value.trim().toLowerCase();const seen=new Set();
  // label matches first (start of the label before the middle), then matches of the path / ID; a toolbar button with the
  // same name as a menu item of this window is the same command and is listed once (the menu item, with its keys)
  const score=c=>{const l=c.label.toLowerCase();return !q?0:l.startsWith(q)?0:l.includes(q)?1:(c.path.toLowerCase().includes(q)||c.id.toLowerCase().includes(q))?2:9};
  const isMenu=c=>c.area.includes('.menu')?0:1;
  shown=allCommands().map(c=>({c,sc:score(c)})).filter(x=>x.sc<9).sort((a,b)=>(a.c.disabled-b.c.disabled)||(a.sc-b.sc)||(isMenu(a.c)-isMenu(b.c))||a.c.path.localeCompare(b.c.path))
   .map(x=>x.c).filter(c=>{const k=c.window+'|'+c.label.replace(/\.\.\.$|…$/,'').toLowerCase();if(isMenu(c)===0){seen.add(k);return true}return !seen.has(k)}).slice(0,200);
  sel=Math.min(sel,Math.max(0,shown.length-1));list.replaceChildren(...(shown.length?shown.map((c,i)=>{const r=el('div',{class:'sh-palrow'+(i===sel?' sel':'')+(c.disabled?' dis':''),role:'option'},
    el('span',{class:'sh-pallabel',text:c.label}),el('span',{class:'sh-palpath',text:c.path}),el('span',{class:'sh-palsc',text:c.sc||''}));
   if(c.disabled)r.title=T('shPaletteDisabled');r.addEventListener('mousedown',e=>{e.preventDefault();sel=i;run()});return r}):[el('div',{class:'sh-palnone',text:T('shPaletteNone')})]));
  const s=list.children[sel];if(s&&s.scrollIntoView)s.scrollIntoView({block:'nearest'})};
 const close=()=>back.remove();
 const run=()=>{const c=shown[sel];if(!c||c.disabled)return;close();setTimeout(()=>{try{c.run()}catch(e){message(String(e.message||e))}},0)};
 inp.addEventListener('input',()=>{sel=0;draw()});
 inp.addEventListener('keydown',e=>{if(e.key==='ArrowDown'){e.preventDefault();sel=Math.min(shown.length-1,sel+1);draw()}else if(e.key==='ArrowUp'){e.preventDefault();sel=Math.max(0,sel-1);draw()}
  else if(e.key==='Enter'){e.preventDefault();run()}else if(e.key==='Escape'){e.preventDefault();e.stopPropagation();close()}});
 back.addEventListener('mousedown',e=>{if(e.target===back)close()});draw();setTimeout(()=>inp.focus())}

// ---------- "Customize interface" editor ----------
function openEditor(){
 const work=clone(layout);let cur=null;let filter='all';let query='';
 const hid=(area,id)=>(work.hidden[area]||[]).includes(id);
 const setHid=(area,id,h)=>{const a=new Set(work.hidden[area]||[]);h?a.add(id):a.delete(id);work.hidden[area]=[...a]};
 const areaList=()=>{const list=[...areas.values()].filter(a=>a.items.length);const w=['main','plot','panels','report'];
  return list.sort((a,b)=>(w.indexOf(a.group)-w.indexOf(b.group))||kindRank(a)-kindRank(b)||a.area.localeCompare(b.area))};
 const kindRank=a=>({menu:a.area.endsWith('.menu')?0:1,tabs:2,toolbar:3,right:4,panels:5,status:6}[a.kind]??7);
 const orderOf=a=>{const o=work.order[a.area];if(!o||!o.length)return a.items.slice();const set=new Set(o);return o.filter(x=>a.items.includes(x)).concat(a.items.filter(x=>!set.has(x)))};
 let tree,table,info,unk;
 dialog(T('shEdTitle'),body=>{body.classList.add('sh-editor');
  const gen=el('div',{class:'sh-edgen'});
  const sel=(label,key,opts)=>{const s=el('select');for(const [v,k] of opts)s.append(new Option(T(k),v));s.value=work[key];s.addEventListener('change',()=>{work[key]=s.value});return el('label',{},el('span',{text:label}),s)};
  gen.append(sel(T('shTheme'),'theme',THEMES.map(t=>[t,'shTheme'+t[0].toUpperCase()+t.slice(1)])),sel(T('shPlotSurface'),'plot_surface',[['auto','shSurfaceAuto'],['light','shSurfaceLight'],['dark','shSurfaceDark']]),
   sel(T('shDensity'),'density',[['compact','shCompact'],['standard','shStandard']]),sel(T('shLabels'),'labels',[['off','shLabelsOff'],['on','shLabelsOn']]),
   sel(T('shChrome'),'chrome',[['industrial','shChromeIndustrial'],['classic','shChromeClassic']]));
  const search=el('input',{type:'search',class:'sh-edsearch',placeholder:T('shEdSearch')});search.addEventListener('input',()=>{query=search.value.trim().toLowerCase();drawTable()});
  const fsel=el('select');for(const [v,k] of [['all','shEdAll'],['shown','shEdShown'],['hidden','shEdHidden'],['unavailable','shEdUnavailable']])fsel.append(new Option(T(k),v));
  fsel.addEventListener('change',()=>{filter=fsel.value;drawTable()});
  tree=el('div',{class:'sh-edtree',role:'tree'});table=el('div',{class:'sh-edtable'});info=el('div',{class:'sh-edinfo'});unk=el('div',{class:'sh-edunk'});
  body.append(gen,el('div',{class:'sh-edtools'},search,fsel),el('div',{class:'sh-edmain'},tree,el('div',{class:'sh-edright'},table,info)),unk,el('p',{class:'note',text:T('shEdNote')}));
  drawTree();drawUnknown()},
  [[T('shEdFactory'),()=>{const keep=work.custom;Object.assign(work,clone(FACTORY));work.custom=keep;drawTree();drawTable();return false}],
   [T('shEdResetArea'),()=>{if(cur){delete work.hidden[cur.area];delete work.order[cur.area];drawTable()}return false}],
   [T('shEdExport'),()=>{const blob=new Blob([JSON.stringify({format:'desktop_ui_layout_v1',...work},null,1)],{type:'application/json'});const a=el('a',{href:URL.createObjectURL(blob),download:'OpenTrace_interface.json'});document.body.append(a);a.click();a.remove();return false}],
   [T('shEdImport'),()=>{const i=el('input',{type:'file',accept:'.json,application/json',style:'display:none'});i.addEventListener('change',async()=>{const f=i.files[0];i.remove();if(!f)return;
     try{const j=JSON.parse(await f.text());if(j.format&&j.format!=='desktop_ui_layout_v1')throw Error(j.format);const v=validate(j);Object.assign(work,v);drawTree();drawTable();drawUnknown();toast(T('shEdImported'))}
     catch(e){message(T('shEdBadFile').replace('{0}',e.message))}});document.body.append(i);i.click();return false}],
   [T('shEdSaveCustom'),()=>{work.custom={hidden:clone(work.hidden),order:clone(work.order)};work.active_preset='custom';toast(T('shPresetCustom'));return false}],
   [T('shEdCancel'),()=>true],
   [T('shEdApply'),()=>{layout=validate(work);layout.custom=work.custom?validate({schema_version:SCHEMA,custom:work.custom}).custom:null;persist();changed('editor');return true},true]]);
 function drawTree(){tree.replaceChildren();let lastGroup=null;
  for(const a of areaList()){if(a.group!==lastGroup){lastGroup=a.group;tree.append(el('div',{class:'sh-edgroup',text:T(WIN_TITLES[a.group]||'shMainWindow')}))}
   const n=el('div',{class:'sh-ednode'+(cur===a?' sel':''),role:'treeitem',text:areaTitle(a)});n.addEventListener('click',()=>{cur=a;drawTree();drawTable()});tree.append(n)}
  for(const w of ['plot','panels','report'])if(![...areas.values()].some(a=>a.group===w))tree.append(el('div',{class:'sh-edgroup dim',text:T(WIN_TITLES[w]),title:T('shEdNotBuilt')}));
  if(!cur){cur=areaList()[0]||null;if(cur)drawTree()}}
 function areaTitle(a){if(a.area.endsWith('.menu'))return T('shMenuArea')+': '+T('shTopMenus');if(a.kind==='menu'){const c=commands.get(a.area.replace(/\.([^.]+)$/,'/$1'));return T('shMenuArea')+': '+(c?c.path:a.title||a.area)}
  return a.title||a.area}
 function drawTable(){if(!cur){table.replaceChildren();return}const a=cur;const ids=orderOf(a);
  const t=el('table',{class:'grid sh-edgrid'});t.append(el('tr',{},...['shEdShow','shEdCommand','shEdPath','shEdKey','shEdState','shEdPos'].map(k=>el('th',{text:T(k)}))));
  ids.forEach((id,i)=>{const c=commands.get(`${a.area}/${id}`)||{label:id,path:'',sc:''};const h=hid(a.area,id);
   if(filter==='shown'&&h)return;if(filter==='hidden'&&!h)return;if(filter==='unavailable'&&!c.disabled)return;
   if(query&&!(String(c.label).toLowerCase().includes(query)||id.toLowerCase().includes(query)||String(c.path).toLowerCase().includes(query)))return;
   const cb=el('input',{type:'checkbox'});cb.checked=!h;if(c.critical){cb.checked=true;cb.disabled=true}
   cb.addEventListener('change',()=>{setHid(a.area,id,!cb.checked)});
   const up=el('button',{type:'button',class:'sh-mini',title:T('shEdUp'),text:'▲'}),dn=el('button',{type:'button',class:'sh-mini',title:T('shEdDown'),text:'▼'});
   const move=d=>{const o=orderOf(a);const j=o.indexOf(id),k=j+d;if(k<0||k>=o.length)return;[o[j],o[k]]=[o[k],o[j]];work.order[a.area]=o;drawTable()};
   up.addEventListener('click',()=>move(-1));dn.addEventListener('click',()=>move(1));
   const state=c.critical?T('shEdCritical'):c.disabled?T('shEdDisabled'):'';
   const tr=el('tr',{draggable:'true'},el('td',{class:'c'},cb),el('td',{text:c.label}),el('td',{class:'dim',text:c.path}),el('td',{text:c.sc||''}),el('td',{class:'dim',text:state}),el('td',{class:'c'},up,dn));
   tr.addEventListener('click',e=>{if(e.target.closest('button,input'))return;info.replaceChildren(el('div',{},el('b',{text:c.label})),el('div',{text:T('shEdId')+': '+`${a.area}/${id}`}),el('div',{text:c.path}),
     c.sc?el('div',{text:T('shEdKey')+': '+c.sc}):'',state?el('div',{text:state}):'')});
   tr.addEventListener('dragstart',e=>{e.dataTransfer.setData('text/plain',id)});tr.addEventListener('dragover',e=>e.preventDefault());
   tr.addEventListener('drop',e=>{e.preventDefault();const from=e.dataTransfer.getData('text/plain');const o=orderOf(a);const j=o.indexOf(from),k=o.indexOf(id);if(j<0||k<0||j===k)return;o.splice(k,0,o.splice(j,1)[0]);work.order[a.area]=o;drawTable()});
   t.append(tr)});
  table.replaceChildren(t)}
 function drawUnknown(){const known=new Set([...commands.keys()]);const bad=[];
  for(const k of ['hidden','order'])for(const [area,ids] of Object.entries(work[k]))if(areas.has(area))for(const id of ids)if(!known.has(`${area}/${id}`))bad.push(`${area}/${id}`);
  unk.replaceChildren();if(!bad.length)return;const b=el('button',{type:'button',class:'btn',text:T('shEdDropUnknown')});
  b.addEventListener('click',()=>{for(const k of ['hidden','order'])for(const area of Object.keys(work[k]))if(areas.has(area))work[k][area]=work[k][area].filter(id=>known.has(`${area}/${id}`));drawUnknown()});
  unk.append(el('span',{text:T('shEdUnknown').replace('{0}',[...new Set(bad)].slice(0,8).join(', '))}),b)}}

// ---------- keys ----------
document.addEventListener('keydown',e=>{if((e.ctrlKey||e.metaKey)&&e.shiftKey&&!e.altKey&&e.code==='KeyP'){e.preventDefault();e.stopImmediatePropagation();openPalette()}
 else if((e.ctrlKey||e.metaKey)&&e.shiftKey&&e.key==='F12'){e.preventDefault();e.stopImmediatePropagation();resetAll(true)}},true);

// ---------- start ----------
applyTheme();
function start(){if(layout.chrome)mountAppbar();bindAll();if(loadProblem)setTimeout(()=>message(T('shCorrupt').replace('{0}',loadProblem)),300);loadDesktop()}
if(document.readyState==='loading')document.addEventListener('DOMContentLoaded',start);else start();

window.Shell={get layout(){return clone(layout)},prepareMenu,bind,bindAll,openEditor,openPalette,resetAll,applyPreset,set,
 setStatusProvider(f){statusProvider=f;renderRight()},refreshRight:renderRight,onChange(f){listeners.push(f)},plotSurface,theme:resolvedTheme,
 commands:()=>[...commands.values()].map(c=>({id:c.id,label:c.label,path:c.path,sc:c.sc,kind:c.kind,area:c.area,window:c.window,disabled:!!c.disabled,critical:!!c.critical,dynamic:!!c.dynamic,hasRun:!!c.run})),
 areas:()=>[...areas.values()].map(a=>({area:a.area,title:a.title,group:a.group,kind:a.kind,items:a.items.slice()})),
 run(id){const c=commands.get(id);if(!c||!c.run)throw Error('no command '+id);if(c.disabled)throw Error('disabled '+id);return c.run()},
 isHidden,validate,FACTORY:clone(FACTORY),PRESETS:clone(PRESETS),win:WIN};
})();
