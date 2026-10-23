// Shared desktop widgets: own SVG icons, PQV symbols, menus, dialogs.
(function(){
const svg=(body,vb='0 0 16 16')=>`<svg viewBox="${vb}" aria-hidden="true">${body}</svg>`;
const ICONS={
 kit:svg('<rect x="1.5" y="3.5" width="13" height="10" rx="1" fill="#f3e3b5" stroke="#8a6d1c"/><path d="M1.5 6.5h13" stroke="#8a6d1c"/><rect x="6" y="2" width="4" height="3" fill="#d9b44a" stroke="#8a6d1c"/>'),
 stutter:svg('<path d="M1 14h14" stroke="#555"/><path d="M3 14V9M6 14V4M10 14V2M13 14V11" stroke="#1f4fd6" stroke-width="2"/>'),
 newProject:svg('<path d="M3 1h7l3 3v11H3z" fill="#fff" stroke="#555"/><path d="M10 1v3h3" fill="#ddd" stroke="#555"/>'),
 openProject:svg('<path d="M1 4h5l1 1h7v9H1z" fill="#f2c94c" stroke="#9a7b1c"/><path d="M1 14l2-6h13l-2 6z" fill="#f8dd7a" stroke="#9a7b1c"/>'),
 saveProject:svg('<rect x="1.5" y="1.5" width="13" height="13" rx="1" fill="#3b6fd6" stroke="#1f3f8a"/><rect x="4" y="2" width="8" height="5" fill="#fff"/><rect x="4" y="10" width="8" height="4" fill="#cfd8ee"/>'),
 addSamples:svg('<path d="M2 1h6l2 2v6H2z" fill="#fff" stroke="#555"/><path d="M5 4h3M5 6h3" stroke="#3a3"/><circle cx="11.5" cy="11.5" r="4" fill="#2e9d3e"/><path d="M11.5 9v5M9 11.5h5" stroke="#fff" stroke-width="1.6"/>'),
 exportTable:svg('<rect x="1.5" y="2.5" width="10" height="11" fill="#fff" stroke="#555"/><path d="M1.5 6h10M1.5 9.5h10M5 2.5v11" stroke="#999"/><path d="M10 8h5m-2-2 2 2-2 2" stroke="#1f6fd6" stroke-width="1.6" fill="none"/>'),
 displayPlots:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M2 13 4 12 5 4 6 12 8 13 9 7 10 13 11 9 12 13h2" stroke="#1f4fd6" fill="none"/>'),
 reportManager:svg('<rect x="2.5" y="1.5" width="11" height="13" fill="#fff" stroke="#555"/><path d="M4 4h8M4 6.5h8M4 9h8M4 11.5h5" stroke="#3a7"/>'),
 labelEditViewer:svg('<rect x="1.5" y="2.5" width="13" height="11" fill="#fff" stroke="#555"/><path d="M3 5h10M3 8h10M3 11h6" stroke="#999"/><path d="M9 12l5-5 1 1-5 5H9z" fill="#e8a33d" stroke="#8a5a14"/>'),
 sizeMatchEditor:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M3 13 13 3" stroke="#d33"/><circle cx="5" cy="11" r="1.2" fill="#e67e22"/><circle cx="8" cy="8" r="1.2" fill="#e67e22"/><circle cx="11" cy="5" r="1.2" fill="#e67e22"/>'),
 analysisMethodEditor:svg('<rect x="1.5" y="2.5" width="13" height="11" fill="#fff" stroke="#555"/><path d="M4 5v6M8 5v6M12 5v6" stroke="#888"/><rect x="2.8" y="6" width="2.4" height="2" fill="#1f4fd6"/><rect x="6.8" y="8" width="2.4" height="2" fill="#1f4fd6"/><rect x="10.8" y="5" width="2.4" height="2" fill="#1f4fd6"/>'),
 panelManager:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M4 3v10M6.5 3v10M9 3v10M11.5 3v10" stroke="#c22" stroke-width="1.4"/>'),
 manager:svg('<rect x="1.5" y="3.5" width="13" height="10" fill="#fff" stroke="#555"/><rect x="1.5" y="1.5" width="5" height="2" fill="#ddd" stroke="#555"/><path d="M3 7h10M3 9.5h10M3 12h6" stroke="#999"/>'),
 analyze:svg('<path d="M4 2l10 6-10 6z" fill="#2ea043" stroke="#176b2b"/>'),
 stop:svg('<path d="M5 1h6l4 4v6l-4 4H5l-4-4V5z" fill="#d93025" stroke="#7a130c"/>'),
 lowQualityTop:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M3 4h4M3 7h6M3 10h8M3 13h10" stroke="#999"/><path d="M12 9V2m-2 2 2-2 2 2" stroke="#d33" fill="none" stroke-width="1.4"/>'),
 tableSettingEditor:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M1.5 5h13M6 1.5v13M10 1.5v13" stroke="#999"/><rect x="1.5" y="1.5" width="13" height="3.5" fill="#cfd8ee" stroke="#555"/>'),
 find:svg('<circle cx="6.5" cy="6.5" r="4.5" fill="#e8f1ff" stroke="#335"/><path d="M10 10l5 5" stroke="#335" stroke-width="2"/>'),
 print:svg('<rect x="4" y="1.5" width="8" height="5" fill="#fff" stroke="#555"/><rect x="1.5" y="6.5" width="13" height="6" rx="1" fill="#bbb" stroke="#555"/><rect x="4" y="10.5" width="8" height="4" fill="#fff" stroke="#555"/>'),
 printPreview:svg('<rect x="2.5" y="1.5" width="9" height="12" fill="#fff" stroke="#555"/><circle cx="10" cy="10" r="3" fill="#e8f1ff" stroke="#335"/><path d="M12 12l3 3" stroke="#335" stroke-width="1.6"/>'),
 help:svg('<circle cx="8" cy="8" r="6.5" fill="#2f6fd6" stroke="#174091"/><path d="M6 6.2c0-1.3 1-2 2.1-2 1.2 0 2 .8 2 1.8 0 1.5-2 1.6-2 3" stroke="#fff" stroke-width="1.5" fill="none"/><circle cx="8.1" cy="11.6" r=".9" fill="#fff"/>'),
 project:svg('<rect x="6" y="1.5" width="4" height="3" fill="#cfd8ee" stroke="#335"/><rect x="1.5" y="11" width="4" height="3" fill="#cfd8ee" stroke="#335"/><rect x="10.5" y="11" width="4" height="3" fill="#cfd8ee" stroke="#335"/><path d="M8 4.5V8M3.5 11V8h9v3" stroke="#335" fill="none"/>'),
 folder:svg('<path d="M1 3.5h5l1 1.5h7.5v8.5H1z" fill="#f2c94c" stroke="#9a7b1c"/>'),
 sample:svg('<path d="M3 1.5h7l3 3v10H3z" fill="#fff" stroke="#556"/><path d="M5 9l1.2-3 1 5 1-3 1 1h2" stroke="#1f4fd6" fill="none"/>'),
 panel:svg('<rect x="2" y="3" width="12" height="10" fill="#fff" stroke="#556"/><path d="M5 3v10M8 3v10M11 3v10" stroke="#c22"/>'),
 marker:svg('<rect x="2" y="6" width="12" height="4" fill="#4caf50" stroke="#2e7d32"/>'),
 combine:svg('<rect x="1.5" y="2.5" width="13" height="11" fill="#fff" stroke="#555"/><path d="M2 12l2-7 1 7 2-4 1 4 2-8 1 8 2-3 1 3" stroke="#1f4fd6" fill="none"/><path d="M2 12l3-4 2 4 3-2 2 2" stroke="#2e9d3e" fill="none"/>'),
 separate:svg('<rect x="1.5" y="1.5" width="13" height="6" fill="#fff" stroke="#555"/><rect x="1.5" y="8.5" width="13" height="6" fill="#fff" stroke="#555"/><path d="M2 7l2-4 1 4 2-3 1 3" stroke="#1f4fd6" fill="none"/><path d="M2 14l3-4 2 4 3-2 2 2" stroke="#2e9d3e" fill="none"/>'),
 overlay:svg('<rect x="1.5" y="2.5" width="13" height="11" fill="#fff" stroke="#555"/><path d="M2 12l3-8 2 8" stroke="#d33" fill="none"/><path d="M5 12l3-6 2 6" stroke="#1f4fd6" fill="none"/><path d="M8 12l3-7 2 7" stroke="#2e9d3e" fill="none"/>'),
 noLabels:svg('<rect x="2.5" y="4.5" width="11" height="7" fill="#fff" stroke="#999" stroke-dasharray="2 1"/>'),
 hLabels:svg('<rect x="1.5" y="5.5" width="13" height="5" fill="#fff" stroke="#555"/><path d="M3 8h10" stroke="#333"/>'),
 vLabels:svg('<rect x="5.5" y="1.5" width="5" height="13" fill="#fff" stroke="#555"/><path d="M8 3v10" stroke="#333"/>'),
 bins:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><rect x="3" y="2" width="2" height="12" fill="#ccc"/><rect x="7" y="2" width="2" height="12" fill="#ccc"/><rect x="11" y="2" width="2" height="12" fill="#f7c6d9"/>'),
 header:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><rect x="2" y="2" width="12" height="3" fill="#cfd8ee"/>'),
 markerRange:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><rect x="2" y="3" width="5" height="3" fill="#4caf50"/><rect x="8" y="3" width="6" height="3" fill="#f5c542"/>'),
 fullView:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M4 4h3M4 4v3M12 12H9M12 12V9M12 4H9M12 4v3M4 12h3M4 12V9" stroke="#333"/>'),
 sizingTable:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M1.5 5h13M1.5 8.5h13M1.5 12h13M6 1.5v13" stroke="#999"/><rect x="1.5" y="1.5" width="13" height="3.5" fill="#f5c542" stroke="#555"/>'),
 genotypesTable:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M1.5 5h13M1.5 8.5h13M1.5 12h13M6 1.5v13" stroke="#999"/><rect x="1.5" y="1.5" width="13" height="3.5" fill="#4caf50" stroke="#555"/>'),
 noTable:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#999" stroke-dasharray="2 1"/>'),
 plotSettingEditor:svg('<rect x="1.5" y="1.5" width="13" height="13" fill="#fff" stroke="#555"/><path d="M2 12l2-6 2 6 2-8 2 8 2-4 2 4" stroke="#1f4fd6" fill="none"/><circle cx="12" cy="4" r="2.5" fill="#999" stroke="#555"/>'),
};
window.icon=name=>ICONS[name]||ICONS.sample;

// PQV symbols: pass = green square, check = yellow triangle, low = red octagon.
const Q={
 pass:svg('<rect x="1.5" y="1.5" width="11" height="11" rx="2" fill="#2fb34a" stroke="#14662a"/><rect x="3" y="3" width="8" height="3" rx="1" fill="#8fe39e"/>','0 0 14 14'),
 check:svg('<path d="M7 1.2l6 11H1z" fill="#f5b400" stroke="#8f6a00" stroke-linejoin="round"/><path d="M7 5v4" stroke="#5a4300" stroke-width="1.4"/><circle cx="7" cy="10.6" r=".8" fill="#5a4300"/>','0 0 14 14'),
 low:svg('<path d="M4.5 1h5l3.5 3.5v5L9.5 13h-5L1 9.5v-5z" fill="#e0221c" stroke="#7a0f0b"/><path d="M4 7h6" stroke="#fff" stroke-width="2"/>','0 0 14 14'),
 override:svg('<rect x="1.5" y="1.5" width="11" height="11" rx="2" fill="#2fb34a" stroke="#14662a"/><path d="M4 7l2 2 4-5" stroke="#fff" stroke-width="1.8" fill="none"/>','0 0 14 14'),
};
window.pqv=(state,edited)=>{
 if(state==null||state==='na')return '<span class="q-na">NA</span>';
 const s=Q[state]||'';
 return `<span class="q" title="${state}" style="${edited?'filter:grayscale(1);opacity:.75':''}">${s}</span>`;
};

// Menus: spec = [{label, items:[{label,sc,run,disabled,checked,sub:[...]}|'-']}]
let openTop=null;
function closeMenus(){document.querySelectorAll('.menu.show').forEach(m=>m.classList.remove('show'));
 document.querySelectorAll('.mtop.open').forEach(m=>m.classList.remove('open'));openTop=null}
function buildMenu(items){
 const menu=document.createElement('div');menu.className='menu';
 for(const it of items){
  if(it==='-'){const s=document.createElement('div');s.className='msep';menu.append(s);continue}
  const row=document.createElement('div');row.className='mi'+(it.disabled?' dis':'')+(it.checked?' check':'')+(it.sub?' sub':'');
  row.textContent=it.label;row.setAttribute('role','menuitem');
  if(it.sc){const k=document.createElement('span');k.className='sc';k.textContent=it.sc;row.append(k)}
  if(it.sub){const sub=buildMenu(it.sub);document.body.append(sub);
   row.addEventListener('mouseenter',()=>{menu.querySelectorAll('.mi.hl').forEach(x=>x.classList.remove('hl'));
    document.querySelectorAll('.menu.sub-open').forEach(x=>{if(x!==sub){x.classList.remove('show','sub-open')}});
    const r=row.getBoundingClientRect();sub.style.left=r.right-2+'px';sub.style.top=r.top-3+'px';sub.classList.add('show','sub-open');row.classList.add('hl')});
  }else row.addEventListener('mouseenter',()=>{document.querySelectorAll('.menu.sub-open').forEach(x=>{if(!menu.contains(x))x.classList.remove('show','sub-open')})});
  if(!it.disabled&&it.run&&!it.sub)row.addEventListener('click',e=>{e.stopPropagation();closeMenus();it.run()});
  menu.append(row);
 }
 return menu;
}
window.buildMenubar=(bar,spec)=>{
 bar.replaceChildren();document.querySelectorAll('body>.menu').forEach(m=>m.remove());
 for(const top of spec){
  const t=document.createElement('div');t.className='mtop';t.textContent=top.label;t.setAttribute('role','menubar');
  const menu=buildMenu(top.items);document.body.append(menu);
  const open=()=>{closeMenus();const r=t.getBoundingClientRect();menu.style.left=r.left+'px';menu.style.top=r.bottom+'px';menu.classList.add('show');t.classList.add('open');openTop=t};
  t.addEventListener('mousedown',e=>{e.stopPropagation();openTop===t?closeMenus():open()});
  t.addEventListener('mouseenter',()=>{if(openTop&&openTop!==t)open()});
  bar.append(t);
 }
};
document.addEventListener('mousedown',e=>{if(!e.target.closest('.menu'))closeMenus()});
document.addEventListener('keydown',e=>{if(e.key==='Escape')closeMenus()});

// Toolbar button
window.tbutton=(name,title,run,opts={})=>{const b=document.createElement('button');b.className='tb';b.type='button';b.title=title;
 b.innerHTML=icon(name);if(run)b.addEventListener('click',run);if(opts.disabled)b.disabled=true;if(opts.id)b.id=opts.id;return b};
window.tsep=()=>{const s=document.createElement('span');s.className='tsep';return s};

// Modal dialog with XP-style caption; returns {root, body, close}
window.dialog=(title,build,buttons)=>{
 const back=document.createElement('div');back.className='backdrop';
 const d=document.createElement('div');d.className='dlg';d.setAttribute('role','dialog');d.setAttribute('aria-label',title);
 const cap=document.createElement('div');cap.className='title';cap.textContent=title;
 const x=document.createElement('span');x.className='x';x.textContent='×';cap.append(x);
 const body=document.createElement('div');body.className='body';
 const foot=document.createElement('div');foot.className='foot';
 d.append(cap,body,foot);back.append(d);document.body.append(back);
 const close=()=>back.remove();x.addEventListener('click',close);
 build(body,close);
 for(const [label,fn,primary] of buttons||[[T('ok'),()=>true,true]]){const b=document.createElement('button');b.className='btn';b.textContent=label;
  b.addEventListener('click',async()=>{if((await fn(body))!==false)close()});foot.append(b);if(primary)setTimeout(()=>b.focus())}
 // drag by caption
 let dx=0,dy=0,drag=false;cap.addEventListener('mousedown',e=>{if(e.target===x)return;drag=true;const r=d.getBoundingClientRect();dx=e.clientX-r.left;dy=e.clientY-r.top;
  d.style.position='fixed';d.style.margin='0'});
 document.addEventListener('mousemove',e=>{if(drag){d.style.left=e.clientX-dx+'px';d.style.top=e.clientY-dy+'px'}});
 document.addEventListener('mouseup',()=>drag=false);
 back.addEventListener('keydown',e=>{if(e.key==='Escape')close()});
 return {root:d,body,close};
};
window.message=(text,title)=>dialog(title||T('app'),b=>{const p=document.createElement('p');p.style.maxWidth='420px';p.textContent=text;b.append(p)});
window.el=(tag,attrs={},...kids)=>{const n=document.createElement(tag);for(const [k,v] of Object.entries(attrs)){if(k==='class')n.className=v;else if(k==='text')n.textContent=v;
 else if(k.startsWith('on'))n.addEventListener(k.slice(2),v);else n.setAttribute(k,v)}n.append(...kids);return n};
})();
