// Users, rights, audit trail and electronic signatures (ID-X Admin menu, login, Reason for Change).
(function(){
const TKEY='opentrace.ticket';
const fmt=(key,...a)=>T(key).replace(/\{(\d)\}/g,(_,i)=>a[+i]);
const getTicket=()=>{try{return localStorage.getItem(TKEY)||''}catch(e){return ''}};
const setTicket=t=>{try{t?localStorage.setItem(TKEY,t):localStorage.removeItem(TKEY)}catch(e){}};
const tz=()=>{try{return Intl.DateTimeFormat().resolvedOptions().timeZone||'UTC'}catch(e){return 'UTC'}};
const download=(name,text,type)=>{const url=URL.createObjectURL(new Blob([text],{type:type||'text/plain'}));const a=el('a',{href:url,download:name});document.body.append(a);a.click();a.remove();setTimeout(()=>URL.revokeObjectURL(url),1000)};
const pickFile=accept=>new Promise(res=>{const i=el('input',{type:'file',accept,style:'display:none'});i.addEventListener('change',async()=>{const f=i.files[0];i.remove();res(f?await f.text():null)});document.body.append(i);i.click()});
const field=(label,input)=>el('div',{class:'field'},el('label',{text:label}),input);
const check=(label,checked,attrs={})=>{const c=el('input',{type:'checkbox',...attrs});c.checked=!!checked;return el('label',{class:'chk'},c,document.createTextNode(' '+label))};
const btn=(label,fn,attrs={})=>{const b=el('button',{class:'btn',type:'button',text:label,...attrs});b.addEventListener('click',fn);return b};
const confirmBox=text=>new Promise(res=>dialog(T('alert'),b=>b.append(el('p',{style:'max-width:440px;white-space:pre-line',text})),[[T('ok'),()=>{res(true);return true},true],[T('cancel'),()=>{res(false);return true}]]));
const when=s=>s?new Date(s).toLocaleString():'';

// Core: one per project window; the plot window borrows the opener's core.
function makeCore(){
 const S={require_login:false,user:null,rights:[],audit:{enabled:false,maps:[],reasons:[],free_text:true},esig:[],policies:{min_length:6},login_users:[]};
 async function api(action,body={}){
  const r=await fetch('/api/security',{method:'POST',headers:{'X-Session-Token':window.OT_TOKEN,'X-User-Ticket':getTicket(),'Content-Type':'application/json'},
   body:JSON.stringify({action,...body})});
  const j=await r.json();if(!r.ok){const e=Error(j.error||'Error');e.denied=!!j.access_denied;throw e}return j}
 function apply(v){for(const k of ['require_login','user','rights','audit','esig','policies','login_users','groups','security_groups','object_rights'])if(k in v)S[k]=v[k];window.dispatchEvent(new Event('opentrace-security'))}
 async function refresh(){apply(await api('session'))}
 return {S,api,apply,refresh}}
const core=(()=>{try{if(window.opener&&window.opener!==window&&window.opener.Sec&&window.opener.Sec.core)return window.opener.Sec.core}catch(e){}return makeCore()})();
const S=core.S,api=core.api;

function can(name){const r=S.rights||[];return r.includes(name)||r.some(k=>k.endsWith('/'+name)&&k.startsWith('GeneMapper ID-X/'))}
function auditState(map,attr){const a=S.audit;if(!a||!a.enabled)return 'off';const m=(a.maps||[]).find(x=>x.name===map);if(!m||!m.enabled)return 'off';
 const at=m.attributes.find(x=>x.name===attr);return !at||!at.auditable?'off':at.state}
function denied(){message(T('scNoRights'))}

// ---------- data items and security groups ----------
// Every analysis method, table setting, size standard, report setting, kit and project belongs to a security group;
// what a user may do with it is the union of his user groups' rights in that group (Security Manager > Security
// Groups). Owners (creator) additionally get owner_update / owner_delete. Browser-side items keep their group and
// owner in one map keyed by kind and name.
const DEFAULT_GROUP='GeneMapper ID-X Security Group';
const IKEY='opentrace.objectGroups.v1';
function groupNames(){return S.security_groups&&S.security_groups.length?S.security_groups:[DEFAULT_GROUP]}
function defaultGroup(){let g='';try{g=window.ProjectOptions?ProjectOptions.get().securityGroup:''}catch(e){}return groupNames().includes(g)?g:DEFAULT_GROUP}
function objectRights(group){const r=S.object_rights||{};if(!Object.keys(r).length)return ['read','update','create','delete','control','owner_update','owner_delete'];
 return r[group]||r[DEFAULT_GROUP]||[]}
function objectCan(group,op,owner){const r=objectRights(group||DEFAULT_GROUP);if(r.includes(op))return true;
 return (op==='update'||op==='delete')&&!!owner&&owner===S.user?.name&&r.includes('owner_'+op)}
function itemsLoad(){try{const v=JSON.parse(localStorage.getItem(IKEY)||'{}');return v&&typeof v==='object'?v:{}}catch(e){return {}}}
function itemsStore(v){try{localStorage.setItem(IKEY,JSON.stringify(v))}catch(e){}}
const items={
 get(kind,name){const v=itemsLoad()[kind]?.[name];return {group:v?.group||DEFAULT_GROUP,owner:v?.owner||''}},
 set(kind,name,group,owner){const v=itemsLoad();(v[kind]=v[kind]||{})[name]={group:group||DEFAULT_GROUP,owner:owner??(S.user?.name||'')};itemsStore(v)},
 rename(kind,from,to){if(from===to)return;const v=itemsLoad();if(v[kind]?.[from]){v[kind][to]=v[kind][from];delete v[kind][from];itemsStore(v)}},
 remove(kind,name){const v=itemsLoad();if(v[kind]?.[name]){delete v[kind][name];itemsStore(v)}},
 can(kind,name,op){const i=items.get(kind,name);return objectCan(i.group,op,i.owner)},
 need(kind,name,op){if(items.can(kind,name,op))return true;message(fmt('scItemDenied',name,items.get(kind,name).group,T('scOp_'+op)));return false}};
// "Choose Security Group" (CSH/Project/Choose_Security_Group): groups in which the user may create items.
function chooseGroup(current){return new Promise(res=>{const usable=groupNames().filter(g=>objectCan(g,'create'));
 if(!usable.length){message(T('scNoCreateGroup'));return res(null)}
 const sel=el('select',{id:'csgSel'});for(const g of usable)sel.append(new Option(g,g));sel.value=usable.includes(current)?current:usable.includes(defaultGroup())?defaultGroup():usable[0];
 dialog(T('scChooseGroup'),b=>{b.classList.add('csgdlg');b.append(el('p',{text:T('scChooseGroupText')}),field(T('securityGroupLbl'),sel))},
  [[T('ok'),()=>{res(sel.value);return true},true],[T('cancel'),()=>{res(null);return true}]])})}
function need(name){if(can(name))return true;denied();return false}

// ---------- login / logout / password ----------
function loginDialog(){return new Promise(res=>{
 const users=S.login_users||[];
 dialog(fmt('scLoginTo','OpenTrace'),b=>{b.classList.add('scdlg');
  const name=el('input',{type:'text',id:'lgName',list:'lgUsers',autocomplete:'username'});const dl=el('datalist',{id:'lgUsers'});for(const u of users)dl.append(el('option',{value:u}));
  try{name.value=localStorage.getItem('opentrace.lastUser')||''}catch(e){}
  b.append(el('div',{class:'lgbrand',text:'OpenTrace'}),field(T('scUserName'),name),dl,field(T('scPassword'),el('input',{type:'password',id:'lgPass',autocomplete:'current-password'})),
   field(T('scDatabaseHost'),el('input',{type:'text',value:location.host,disabled:''})),el('p',{class:'note',text:T('scCaseNote')}));
  setTimeout(()=>(name.value?b.querySelector('#lgPass'):name).focus(),0);
  b.addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();b.closest('.dlg').querySelector('.foot .btn').click()}})},
  [[T('ok'),async b=>{const n=b.querySelector('#lgName').value.trim(),p=b.querySelector('#lgPass').value;
    if(!n){message(T('scEnterUser'));return false}
    try{const r=await api('login',{name:n,password:p});try{localStorage.setItem('opentrace.lastUser',n)}catch(e){}
     if(r.must_change_password){message(T('scExpired'));const ok=await changePassword(n,true);if(ok){res(await loginDialog())}else res(false);return true}
     setTicket(r.ticket);core.apply(r);res(true);return true}catch(e){message(e.message);return false}},true],
   [T('scExit'),()=>{res(false);return true}]]);
 // the login dialog cannot be dismissed without a decision
 const back=[...document.querySelectorAll('.backdrop')].pop();back?.querySelector('.title .x')?.remove()})}
async function init(){
 try{await core.refresh()}catch(e){return}
 if(S.require_login&&!S.user){setTicket('');for(;;){const ok=await loginDialog();if(ok)break;document.body.replaceChildren(el('div',{class:'loggedout'},el('p',{text:T('scLoggedOut')}),btn(T('scLogIn'),()=>location.reload())));return false}}
 return true}
async function logout(){try{await api('logout')}catch(e){}setTicket('');await core.refresh();
 if(S.require_login){const ok=await loginDialog();if(!ok){document.body.replaceChildren(el('div',{class:'loggedout'},el('p',{text:T('scLoggedOut')}),btn(T('scLogIn'),()=>location.reload())))}}}
function changePassword(name,forced){return new Promise(res=>{const who=name||S.user?.name||'';
 dialog(T('scChangePassword'),b=>{b.append(el('p',{text:fmt('scChangeFor',who)}),field(T('scOldPassword'),el('input',{type:'password',id:'cpOld'})),
   field(T('scPassword'),el('input',{type:'password',id:'cpNew'})),field(T('scConfirmPassword'),el('input',{type:'password',id:'cpConf'})),
   el('p',{class:'note',text:fmt('scMinLength',S.policies?.min_length??6)}))},
  [[T('ok'),async b=>{const o=b.querySelector('#cpOld').value,n=b.querySelector('#cpNew').value,c=b.querySelector('#cpConf').value;
    if(n!==c){message(T('scNotConsistent'));return false}
    try{await api('change_password',{name:who,old:o,new:n});message(T('scPasswordChanged'));res(true);return true}catch(e){message(e.message);return false}},true],
   [T('cancel'),()=>{res(false);return true}]])})}

// ---------- Reason for Change ----------
function reasonDialog(rows,title){return new Promise(res=>{const a=S.audit||{};const reasons=(a.reasons||[]).filter(r=>r.active);
 dialog(title||T('scReasonTitle'),b=>{b.classList.add('rfcdlg');
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...[T('scAttribute'),T('scOldValue'),T('scNewValue')].map(h=>el('th',{text:h})))));
  const tb=el('tbody');for(const r of rows)tb.append(el('tr',{},el('td',{text:r.attribute}),el('td',{text:r.old??''}),el('td',{text:r.new??''})));t.append(tb);
  b.append(el('div',{class:'grid rfcgrid'},t));
  let input;if(a.free_text!==false){input=el('input',{type:'text',id:'rfcReason',list:'rfcList',maxlength:'500'});const dl=el('datalist',{id:'rfcList'});for(const r of reasons)dl.append(el('option',{value:r.reason}));b.append(field(T('scEnterReason'),input),dl)}
  else{input=el('select',{id:'rfcReason'});input.append(new Option('',''));for(const r of reasons){const o=new Option(r.reason,r.reason);o.title=r.description||'';input.append(o)}b.append(field(T('scEnterReason'),input))}
  setTimeout(()=>input.focus(),0)},
  [[T('ok'),b=>{const v=b.querySelector('#rfcReason').value.trim();if(!v){message(T('scMustSpecify'));return false}res(v);return true},true],[T('cancel'),()=>{res(null);return true}]])})}

// ---------- electronic signature ----------
function esigGroup(name){return (S.esig||[]).find(g=>g.name===name)}
function esign(group,action){const g=esigGroup(group);if(!g||!g.required)return Promise.resolve(true);
 const a=g.actions.find(x=>x.name===action);
 return new Promise(res=>{dialog(T('scEsigTitle'),b=>{b.classList.add('scdlg');
   b.append(el('p',{class:'esigch',text:g.challenge}),el('p',{},el('b',{text:T('scMeaning')+' '}),document.createTextNode(a?a.meaning:action)),
    field(T('scUserName'),el('input',{type:'text',id:'esName',value:S.user?.name||''})),field(T('scPassword'),el('input',{type:'password',id:'esPass'})));
   setTimeout(()=>b.querySelector('#esPass').focus(),0)},
  [[T('ok'),async b=>{try{const r=await api('verify',{name:b.querySelector('#esName').value.trim(),password:b.querySelector('#esPass').value,group,action_name:action});
     message(fmt('scSigned',r.meaning));res(true);return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>{res(false);return true}]])})}

// change(spec): e-signature (if required) + reason (if the audit state is On); returns {reason, commit()} or null when cancelled.
async function change(spec){
 if(spec.esig&&!(await esign(spec.esig.group,spec.esig.action)))return null;
 const items=(spec.items||[spec]).map(x=>({...x,state:auditState(x.map,x.attribute)}));
 let reason=spec.reason||'';
 if(!reason&&items.some(x=>x.state==='on')){reason=await reasonDialog(items.filter(x=>x.state==='on').map(x=>({attribute:x.label||`${x.map}.${x.attribute}`,old:x.old,new:x.new})),spec.title);if(reason==null)return null}
 return {reason,commit:async(extra)=>{const recs=items.filter(x=>x.state!=='off').map(x=>({map:x.map,instance:x.instance,attribute:x.attribute,old:x.old??'',new:x.new??'',reason,event:x.event||'',...(extra||{})}));
  if(recs.length)try{await api('record',{records:recs,tz:tz()})}catch(e){message(e.message)}}}}
async function record(items){const recs=items.filter(x=>auditState(x.map,x.attribute)!=='off');if(!recs.length)return;
 try{await api('record',{records:recs,tz:tz()})}catch(e){console.warn(e)}}

// ---------- Security Manager ----------
async function securityManager(){
 let st;try{st=await api('admin_state')}catch(e){return message(e.denied?T('scNoRights'):e.message)}
 let sel={kind:'users',name:st.users[0]?.name};
 const d=dialog(T('scSecurityManager'),b=>b.classList.add('secdlg'),[[T('scDone'),()=>true,true]]);
 const reload=async(keep)=>{st=await api('admin_state');if(keep)sel=keep;draw()};
 const KINDS=[['users','scUsers',()=>st.users],['groups','scUserGroups',()=>st.user_groups],['profiles','scProfiles',()=>st.profiles],['sgroups','scSecurityGroups',()=>st.security_groups]];
 const draw=()=>{const b=d.body;b.replaceChildren();
  const tree=el('div',{class:'sectree'});
  for(const [k,label,list] of KINDS){const h=el('div',{class:'stnode'+(sel.kind===k&&!sel.name?' sel':''),text:T(label)});h.addEventListener('mousedown',()=>{sel={kind:k,name:null};draw()});tree.append(h);
   for(const x of list()){const n=el('div',{class:'stleaf'+(sel.kind===k&&sel.name===x.name?' sel':''),text:x.name,'data-name':x.name});
    if(k==='users'&&x.status!=='active')n.classList.add('dim');n.addEventListener('mousedown',()=>{sel={kind:k,name:x.name};draw()});tree.append(n)}}
  const props=el('div',{class:'secprops'});
  const tools=el('div',{class:'btnrow'},btn(T('scNewUser'),()=>{sel={kind:'users',name:null,isNew:true};draw()},{id:'scNewUserBtn'}),btn(T('scNewGroup'),()=>{sel={kind:'groups',name:null,isNew:true};draw()}),
   btn(T('scNewProfile'),()=>{sel={kind:'profiles',name:null,isNew:true};draw()}),btn(T('scNewSecurityGroup'),()=>{sel={kind:'sgroups',name:null,isNew:true};draw()}),
   btn(T('scPasswordPolicies'),policies,{id:'scPoliciesBtn'}),btn(T('scSettings'),settings,{id:'scSettingsBtn'}),
   btn(T('scExport'),async()=>{try{download('OpenTrace security.json',(await api('export')).text,'application/json')}catch(e){message(e.message)}}),
   btn(T('scImport'),async()=>{const text=await pickFile('.json');if(!text)return;if(!(await confirmBox(T('scImportWarn'))))return;
    try{const r=await api('import',{text});message(fmt('scImported',r.users));await core.refresh();reload({kind:'users',name:null})}catch(e){message(e.message)}}));
  if(sel.kind==='users'&&(sel.name||sel.isNew))props.append(userPanel(st.users.find(u=>u.name===sel.name)));
  else if(sel.kind==='groups'&&(sel.name||sel.isNew))props.append(groupPanel(st.user_groups.find(g=>g.name===sel.name)));
  else if(sel.kind==='profiles'&&(sel.name||sel.isNew))props.append(profilePanel(st.profiles.find(p=>p.name===sel.name)));
  else if(sel.kind==='sgroups'&&(sel.name||sel.isNew))props.append(sgroupPanel(st.security_groups.find(g=>g.name===sel.name)));
  else props.append(listPanel());
  b.append(tools,el('div',{class:'secmain'},tree,props))};
 const listPanel=()=>{const k=sel.kind;const t=el('table',{class:'g'});
  if(k==='users'){t.append(el('thead',{},el('tr',{},...['scUserName','scFullName','scProfile','scUserGroups','scStatus','scLastLogin'].map(h=>el('th',{text:T(h)})))));
   const tb=el('tbody');for(const u of st.users)tb.append(el('tr',{},el('td',{text:u.name}),el('td',{text:u.full_name||''}),el('td',{text:u.profile}),el('td',{text:(u.groups||[]).join(', ')}),
    el('td',{text:T('scSt_'+u.status)+(u.has_password?'':' · '+T('scNoPassword'))}),el('td',{text:when(u.last_login)})));t.append(tb)}
  else if(k==='groups'){t.append(el('thead',{},el('tr',{},el('th',{text:T('scGroupName')}),el('th',{text:T('scDescription')}),el('th',{text:T('scMembers')}))));
   const tb=el('tbody');for(const g of st.user_groups)tb.append(el('tr',{},el('td',{text:g.name}),el('td',{text:g.description||''}),el('td',{text:g.members.join(', ')})));t.append(tb)}
  else if(k==='profiles'){t.append(el('thead',{},el('tr',{},el('th',{text:T('scName')}),el('th',{text:T('scDescription')}))));
   const tb=el('tbody');for(const p of st.profiles)tb.append(el('tr',{},el('td',{text:p.name}),el('td',{text:p.description||''})));t.append(tb)}
  else{t.append(el('thead',{},el('tr',{},el('th',{text:T('scName')}),el('th',{text:T('scDescription')}),el('th',{text:T('scUserGroups')}))));
   const tb=el('tbody');for(const g of st.security_groups)tb.append(el('tr',{},el('td',{text:g.name}),el('td',{text:g.description||''}),el('td',{text:Object.keys(g.rights).join(', ')})));t.append(tb)}
  return el('div',{class:'grid'},t)};
 const save=async(action,payload,next)=>{try{await api(action,payload);await core.refresh();await reload(next)}catch(e){message(e.message)}};
 const del=async(action,name,kind)=>{if(!name)return;if(!(await confirmBox(fmt('scConfirmDelete',name))))return;await save(action,{name},{kind,name:null})};
 function userPanel(u){const isNew=!u;u=u||{name:'',full_name:'',description:'',profile:'Analyst',groups:[],disabled:false,status:'active'};
  const prof=el('select',{id:'scuProfile'});for(const p of st.profiles)prof.append(new Option(p.name,p.name));prof.value=u.profile;
  const groups=el('div',{class:'cklist'});for(const g of st.user_groups)if(!g.all_users)groups.append(check(g.name,u.groups.includes(g.name),{'data-group':g.name}));
  const box=el('div',{class:'secform'},el('h3',{text:isNew?T('scCreateUser'):fmt('scDetailsUser',u.name)}),
   field(T('scUserName'),el('input',{type:'text',id:'scuName',value:u.name,maxlength:'30'})),field(T('scFullName'),el('input',{type:'text',id:'scuFull',value:u.full_name||''})),
   field(T('scDescription'),el('input',{type:'text',id:'scuDesc',value:u.description||''})),field(T('scProfile'),prof),
   field(T('scPassword'),el('input',{type:'password',id:'scuPass',autocomplete:'new-password',placeholder:isNew||!u.has_password?T('scSetPassword'):T('scKeepPassword')})),
   field(T('scConfirmPassword'),el('input',{type:'password',id:'scuConf',autocomplete:'new-password'})),
   check(T('scDisabled'),u.disabled,{id:'scuDisabled'}),
   el('div',{class:'note',text:isNew?'':`${T('scStatus')}: ${T('scSt_'+u.status)}${u.has_password?'':' · '+T('scNoPassword')} · ${T('scCreated')}: ${when(u.created)} · ${T('scLastLogin')}: ${when(u.last_login)||'—'}`}),
   el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scUserGroups')}),groups,el('div',{class:'note',text:T('scAllUsersNote')})));
  const unlock=u.status==='suspended'?btn(T('scUnlock'),()=>save('save_user',{previous:u.name,user:{...u,unlock:true}},{kind:'users',name:u.name})):null;
  box.append(el('div',{class:'btnrow'},btn(T('scSave'),()=>{const q=s=>box.querySelector(s);const p=q('#scuPass').value;if(p!==q('#scuConf').value)return message(T('scNotConsistent'));
    save('save_user',{previous:isNew?null:u.name,user:{name:q('#scuName').value.trim(),full_name:q('#scuFull').value.trim(),description:q('#scuDesc').value.trim(),profile:q('#scuProfile').value,
     disabled:q('#scuDisabled').checked,password:p||undefined,groups:[...box.querySelectorAll('[data-group]')].filter(c=>c.checked).map(c=>c.dataset.group)}},{kind:'users',name:q('#scuName').value.trim()})},{id:'scuSave'}),
   ...(unlock?[unlock]:[]),...(isNew?[]:[btn(T('scDelete'),()=>del('delete_user',u.name,'users'))])));
  if(u.name==='Administrator')box.append(el('p',{class:'note',text:T('scAdminNote')}));return box}
 function groupPanel(g){const isNew=!g;g=g||{name:'',description:'',members:[],all_users:false};
  const mem=el('div',{class:'cklist'});for(const u of st.users)mem.append(check(u.name,g.members.includes(u.name),{'data-user':u.name,...(g.all_users?{disabled:''}:{})}));
  const box=el('div',{class:'secform'},el('h3',{text:isNew?T('scCreateGroup'):fmt('scDetailsGroup',g.name)}),field(T('scGroupName'),el('input',{type:'text',id:'scgName',value:g.name,maxlength:'30'})),
   field(T('scDescription'),el('input',{type:'text',id:'scgDesc',value:g.description||''})),el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scMembers')}),mem));
  box.append(el('div',{class:'btnrow'},btn(T('scSave'),()=>{const n=box.querySelector('#scgName').value.trim();
   save('save_group',{previous:isNew?null:g.name,group:{name:n,description:box.querySelector('#scgDesc').value.trim(),members:[...box.querySelectorAll('[data-user]')].filter(c=>c.checked).map(c=>c.dataset.user)}},{kind:'groups',name:n})}),
   ...(isNew?[]:[btn(T('scDelete'),()=>del('delete_group',g.name,'groups'))])));return box}
 function profilePanel(p){const isNew=!p;p=p?JSON.parse(JSON.stringify(p)):{name:'',description:'',default_enable:false,rights:{}};
  const box=el('div',{class:'secform'},el('h3',{text:isNew?T('scCreateProfile'):fmt('scDetailsProfile',p.name)}),field(T('scName'),el('input',{type:'text',id:'scpName',value:p.name})),
   field(T('scDescription'),el('input',{type:'text',id:'scpDesc',value:p.description||''})),check(T('scDefaultEnable'),p.default_enable,{id:'scpDefault'}));
  const effective=key=>{const [prod,mod]=key.split('/');for(const k of [key,`${prod}/${mod}`,prod])if(k in p.rights)return p.rights[k];return p.default_enable};
  const t=el('table',{class:'g rights'});t.append(el('thead',{},el('tr',{},el('th',{text:T('scAccessElement')}),el('th',{text:T('scRights')}),el('th',{text:T('scEffective')}))));
  const tb=el('tbody');t.append(tb);
  const redraw=()=>{tb.replaceChildren();let lastProd=null;
   const row=(key,label,level)=>{const s=el('select',{'data-key':key});for(const [v,k] of [['','scInherit'],['1','scAllow'],['0','scDeny']])s.append(new Option(T(k),v));
    s.value=key in p.rights?(p.rights[key]?'1':'0'):'';s.addEventListener('change',()=>{if(s.value==='')delete p.rights[key];else p.rights[key]=s.value==='1';redraw()});
    const eff=level===2?(effective(key)?'✓':'✗'):'';tb.append(el('tr',{class:'lv'+level},el('td',{style:`padding-left:${6+level*16}px`,text:label}),el('td',{},s),el('td',{class:'eff',text:eff})))};
   for(const c of st.catalog){if(c.product!==lastProd){row(c.product,c.product,0);lastProd=c.product}row(`${c.product}/${c.module}`,c.module,1);
    for(const f of c.functions)row(`${c.product}/${c.module}/${f}`,f,2)}};
  box.querySelector('#scpDefault').addEventListener('change',e=>{p.default_enable=e.target.checked;redraw()});redraw();
  box.append(el('div',{class:'grid rightsgrid'},t),el('div',{class:'btnrow'},btn(T('scSave'),()=>{const n=box.querySelector('#scpName').value.trim();
   save('save_profile',{previous:isNew?null:p.name,profile:{name:n,description:box.querySelector('#scpDesc').value.trim(),default_enable:p.default_enable,rights:p.rights}},{kind:'profiles',name:n})}),
   ...(isNew?[]:[btn(T('scDelete'),()=>del('delete_profile',p.name,'profiles'))])));return box}
 function sgroupPanel(g){const isNew=!g;g=g?JSON.parse(JSON.stringify(g)):{name:'',description:'',rights:{}};
  const R=['read','update','create','delete','control','owner_update','owner_delete'];
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},el('th',{text:T('scUserGroup')}),...R.map(r=>el('th',{text:T('scR_'+r),title:T('scRt_'+r)})))));
  const tb=el('tbody');for(const ug of st.user_groups){const cur=g.rights[ug.name]||{};tb.append(el('tr',{'data-ug':ug.name},el('td',{text:ug.name}),...R.map(r=>{const c=el('input',{type:'checkbox','data-r':r});c.checked=!!cur[r];return el('td',{class:'c'},c)})))}
  t.append(tb);
  const box=el('div',{class:'secform'},el('h3',{text:isNew?T('scCreateSecurityGroup'):fmt('scDetailsSG',g.name)}),field(T('scName'),el('input',{type:'text',id:'scsName',value:g.name})),
   field(T('scDescription'),el('input',{type:'text',id:'scsDesc',value:g.description||''})),el('div',{class:'grid'},t),el('p',{class:'note',text:T('scSgNote')}));
  box.append(el('div',{class:'btnrow'},btn(T('scSave'),()=>{const rights={};for(const tr of tb.children){const r={};let any=false;for(const c of tr.querySelectorAll('[data-r]')){r[c.dataset.r]=c.checked;any=any||c.checked}if(any)rights[tr.dataset.ug]=r}
   const n=box.querySelector('#scsName').value.trim();save('save_security_group',{previous:isNew?null:g.name,group:{name:n,description:box.querySelector('#scsDesc').value.trim(),rights}},{kind:'sgroups',name:n})})));return box}
 function policies(){const p=st.policies;const num=(id,v)=>el('input',{type:'text',class:'num',id,value:String(v)});
  dialog(T('scPasswordPolicies'),b=>{b.classList.add('poldlg');b.append(el('p',{text:T('scPolicyIntro')}),
   el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scMaxAttempts')}),el('div',{},num('poMax',p.max_failed_attempts),document.createTextNode(' '+T('scAttempts'))),
    el('div',{},el('b',{text:T('scUponFailure')+' '}),...['remain_active','disable','suspend'].map(v=>{const r=el('input',{type:'radio',name:'poUpon',value:v});r.checked=p.upon_failure===v;return el('label',{class:'chk'},r,document.createTextNode(' '+T('scUpon_'+v)+' '))}),
     num('poSusp',p.suspension_minutes),document.createTextNode(' '+T('scMins'))),check(T('scSendLog'),p.send_log_message,{id:'poLog'})),
   el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scPwdLifetime')}),check(T('scLifetimeOn'),p.lifetime_enabled,{id:'poLife'}),el('div',{},num('poDays',p.lifetime_days),document.createTextNode(' '+T('scDays'))),
    el('div',{},el('b',{text:T('scGraceLogins')+' '}),num('poGrace',p.grace_logins))),
   el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scPwdReuse')}),check(T('scReuseRestricted'),p.reuse_restricted,{id:'poReuse'}),
    el('div',{},el('b',{text:T('scKept')+' '}),num('poKept',p.passwords_kept)),el('div',{},el('b',{text:T('scReusePeriod')+' '}),num('poPeriod',p.reuse_period_days),document.createTextNode(' '+T('scDays')))),
   el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scPwdFormat')}),el('div',{},el('b',{text:T('scMinLen')+' '}),num('poMin',p.min_length),document.createTextNode(' '+T('scChars')))))},
  [[T('scSaveChanges'),async b=>{const q=s=>b.querySelector(s);try{st.policies=await api('save_policies',{policies:{max_failed_attempts:+q('#poMax').value,upon_failure:q('[name=poUpon]:checked').value,
    suspension_minutes:+q('#poSusp').value,send_log_message:q('#poLog').checked,lifetime_enabled:q('#poLife').checked,lifetime_days:+q('#poDays').value,grace_logins:+q('#poGrace').value,
    reuse_restricted:q('#poReuse').checked,passwords_kept:+q('#poKept').value,reuse_period_days:+q('#poPeriod').value,min_length:+q('#poMin').value}});await core.refresh();return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]])}
 function settings(){dialog(T('scSettings'),b=>b.append(check(T('scRequireLogin'),st.settings.require_login,{id:'stReq'}),el('p',{class:'note',style:'max-width:420px',text:T('scRequireNote')})),
  [[T('ok'),async b=>{try{const r=await api('save_settings',{settings:{require_login:b.querySelector('#stReq').checked}});st.settings=r.settings;if(r.ticket)setTicket(r.ticket);await core.refresh();return true}catch(e){message(e.message);return false}},true],[T('cancel'),()=>true]])}
 draw();return d}

// ---------- Audit Map Configuration (Admin > Audit Manager > Setting) ----------
async function auditSettings(){
 let a;try{a=await api('audit_config')}catch(e){return message(e.denied?T('scNoRights'):e.message)}
 a=JSON.parse(JSON.stringify(a));let cur=a.maps[0]?.name;
 const d=dialog(T('scAuditMapConfig'),b=>b.classList.add('auddlg'),[[T('ok'),async()=>{try{await api('save_audit',{audit:{enabled:a.enabled,maps:a.maps,reasons:a.reasons,free_text:a.free_text}});await core.refresh();return true}catch(e){message(e.message);return false}},true],
  [T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();
  const on=check(T('scAuditTrailOn'),a.enabled,{id:'auOn'});on.querySelector('input').addEventListener('change',e=>{a.enabled=e.target.checked});
  const mt=el('table',{class:'g'});mt.append(el('thead',{},el('tr',{},el('th',{text:T('scName'),title:T('scNameOfMap')}),el('th',{text:T('scType'),title:T('scTypeOfMap')}),el('th',{text:T('scEnabled'),title:T('scMapEnabledTip')}))));
  const mtb=el('tbody');for(const m of a.maps){const c=el('input',{type:'checkbox'});c.checked=m.enabled;c.addEventListener('change',()=>{m.enabled=c.checked});
   const tr=el('tr',{class:m.name===cur?'sel':'','data-map':m.name},el('td',{text:m.name}),el('td',{text:m.type}),el('td',{class:'c'},c));tr.addEventListener('mousedown',e=>{if(e.target!==c){cur=m.name;draw()}});mtb.append(tr)}mt.append(mtb);
  const m=a.maps.find(x=>x.name===cur);const at=el('table',{class:'g'});
  at.append(el('thead',{},el('tr',{},el('th',{text:T('scName')}),el('th',{text:T('scType')}),el('th',{text:T('scState')}),el('th',{text:T('scAuditable')}))));
  const atb=el('tbody');for(const x of m?.attributes||[]){const s=el('select',{'data-attr':x.name});for(const v of ['on','off','silent']){const o=new Option(T('scState_'+v),v);o.title=T('scStateTip_'+v);s.append(o)}s.value=x.state;s.addEventListener('change',()=>{x.state=s.value});
   const c=el('input',{type:'checkbox'});c.checked=x.auditable;c.addEventListener('change',()=>{x.auditable=c.checked});atb.append(el('tr',{},el('td',{text:x.name}),el('td',{text:x.type}),el('td',{},s),el('td',{class:'c'},c)))}at.append(atb);
  const rt=el('table',{class:'g'});rt.append(el('thead',{},el('tr',{},el('th',{text:T('scReason')}),el('th',{text:T('scDescription')}),el('th',{text:T('scInactive')}))));
  const rtb=el('tbody');a.reasons.forEach((r,i)=>{const c=el('input',{type:'checkbox'});c.checked=!r.active;c.addEventListener('change',()=>{r.active=!c.checked});
   const tr=el('tr',{},el('td',{text:r.reason}),el('td',{text:r.description||''}),el('td',{class:'c'},c));tr.addEventListener('dblclick',()=>reasonEditor(r,i));rtb.append(tr)});rt.append(rtb);
  const ft=check(T('scAllowUserReasons'),a.free_text,{id:'auFree'});ft.querySelector('input').addEventListener('change',e=>{a.free_text=e.target.checked});
  b.append(el('div',{class:'btnrow'},on,btn(T('scExport'),async()=>{try{download('Audit Maps.xml',(await api('export_maps')).xml,'application/xml')}catch(e){message(e.message)}}),
    btn(T('scImport'),async()=>{const xml=await pickFile('.xml');if(!xml)return;try{const r=await api('parse_maps',{xml});a.enabled=r.enabled;
     for(const im of r.maps){const t=a.maps.find(x=>x.name===im.name);if(!t)continue;t.enabled=im.enabled;for(const ia of im.attributes){const x=t.attributes.find(y=>y.name===ia.name);if(x&&['on','off','silent'].includes(ia.state)){x.state=ia.state;x.auditable=ia.auditable}}}
     draw();message(T('scMapsImported'))}catch(e){message(e.message)}})),
   el('div',{class:'audmain'},el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scAuditMapObjects')}),el('div',{class:'grid'},mt)),
    el('div',{class:'fieldset'},el('div',{class:'legend',text:fmt('scAttributesFor',cur||'')}),el('div',{class:'grid'},at))),
   el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scAuditReasons')}),el('div',{class:'grid reasons'},rt),el('div',{class:'btnrow'},btn(T('scNew'),()=>reasonEditor(null,-1)),
    btn(T('scOpen'),()=>{const i=[...rtb.children].findIndex(tr=>tr.classList.contains('sel'));if(i>=0)reasonEditor(a.reasons[i],i)}),ft)));
  for(const tr of rtb.children)tr.addEventListener('mousedown',()=>{for(const x of rtb.children)x.classList.toggle('sel',x===tr)})};
 const reasonEditor=(r,i)=>dialog(T('scReasonEditor'),b=>b.append(field(T('scReason'),el('input',{type:'text',id:'reText',value:r?.reason||''})),field(T('scDescription'),el('input',{type:'text',id:'reDesc',value:r?.description||''})),
   check(T('scInactive'),r?!r.active:false,{id:'reInactive'})),
  [[T('ok'),b=>{const t=b.querySelector('#reText').value.trim();if(!t){message(T('scReasonRequired'));return false}
    if(a.reasons.some((x,j)=>j!==i&&x.reason.toLowerCase()===t.toLowerCase())){message(T('scReasonExists'));return false}
    const v={reason:t,description:b.querySelector('#reDesc').value.trim(),active:!b.querySelector('#reInactive').checked};if(i>=0)a.reasons[i]=v;else a.reasons.push(v);draw();return true},true],[T('cancel'),()=>true]]);
 draw();return d}

// ---------- Audit History Viewer (Admin > Audit Manager > History) ----------
const HCOLS=[['time','scDate'],['user','scUser'],['full_name','scFullName'],['map','scObject'],['instance','scInstance'],['attribute','scAttribute'],['old','scOldValue'],['new','scNewValue'],['reason','scReason'],['event','scEvent'],['tz','scTimeZone'],['verified','scVerification']];
async function auditHistory(){
 const q={map:'',instance:'',user:'',attribute:'',text:'',match_case:false,whole_word:false,from:'',to:''};let res,selRec=null,selInst=null;
 const run=async()=>{try{res=await api('history',{query:{...q,instance:selInst?.instance||q.instance,map:selInst?.map||q.map}})}catch(e){message(e.denied?T('scNoRights'):e.message);return false}return true};
 if(!(await run()))return;
 const d=dialog(T('scAuditHistory'),b=>b.classList.add('histdlg'),[[T('scClose'),()=>true,true]]);
 const cell=(r,k)=>k==='time'?new Date(r.time).toLocaleString():k==='verified'?(r.verified?T('scVerified'):T('scNotVerified')):String(r[k]??'');
 const csv=()=>[HCOLS.map(([,h])=>T(h)),...res.records.map(r=>HCOLS.map(([k])=>cell(r,k)))].map(row=>row.map(v=>/[",\n]/.test(v)?'"'+v.replace(/"/g,'""')+'"':v).join(',')).join('\r\n');
 const printView=()=>{const w=window.open('','opentrace-audit-print','width=1000,height=760');if(!w)return;const doc=w.document;doc.title=T('scAuditReport');
  const t=doc.createElement('table');t.setAttribute('border','1');t.style.borderCollapse='collapse';t.style.font='11px Tahoma,sans-serif';
  const hr=doc.createElement('tr');for(const [,h] of HCOLS){const th=doc.createElement('th');th.textContent=T(h);hr.append(th)}t.append(hr);
  for(const r of res.records){const tr=doc.createElement('tr');for(const [k] of HCOLS){const td=doc.createElement('td');td.textContent=cell(r,k);tr.append(td)}t.append(tr)}
  const h=doc.createElement('h3');h.textContent=`${T('scAuditReport')} — ${new Date().toLocaleString()}${(q.text||q.user||q.from||q.to||selInst)?' '+T('scFiltered'):''}`;h.style.font='bold 14px Tahoma,sans-serif';
  doc.body.replaceChildren(h,t);setTimeout(()=>w.print(),200)};
 const draw=()=>{const b=d.body;b.replaceChildren();
  const users=el('select',{id:'ahUser'});users.append(new Option(T('scAll'),''));for(const u of res.users)users.append(new Option(u,u));users.value=q.user;
  const maps=el('select',{id:'ahMap'});maps.append(new Option(T('scAll'),''));for(const m of res.maps)maps.append(new Option(m,m));maps.value=q.map;
  const inp=(id,v,type='text')=>el('input',{type,id,value:v});
  const filt=el('div',{class:'fieldset ahfilter'},el('div',{class:'legend',text:T('scFilterRecords')}),
   el('div',{class:'row'},el('b',{text:T('scByDate')}),el('label',{text:T('scAfter')}),inp('ahFrom',q.from,'date'),el('label',{text:T('scBefore')}),inp('ahTo',q.to,'date'),
    el('b',{text:T('scObject')}),maps,el('b',{text:T('scUser')}),users),
   el('div',{class:'row'},el('b',{text:T('scByText')}),el('label',{text:T('scFindWhat')}),inp('ahText',q.text),check(T('scMatchCase'),q.match_case,{id:'ahCase'}),check(T('scWholeWord'),q.whole_word,{id:'ahWhole'}),
    el('label',{text:T('scAttribute')}),inp('ahAttr',q.attribute),btn(T('scQuery'),async()=>{const g=s=>b.querySelector(s);Object.assign(q,{from:g('#ahFrom').value,to:g('#ahTo').value,map:g('#ahMap').value,user:g('#ahUser').value,
     text:g('#ahText').value,match_case:g('#ahCase').checked,whole_word:g('#ahWhole').checked,attribute:g('#ahAttr').value.trim()});selInst=null;if(await run())draw()},{id:'ahQuery'})));
  const il=el('div',{class:'ahinst'},el('div',{class:'legend',text:T('scInstances')}));
  const all=el('div',{class:'stleaf'+(selInst?'':' sel'),text:fmt('scAllInstances',res.total)});all.addEventListener('mousedown',async()=>{selInst=null;if(await run())draw()});il.append(all);
  for(const x of res.instances){const n=el('div',{class:'stleaf'+(selInst&&selInst.map===x.map&&selInst.instance===x.instance?' sel':''),text:`${x.map}: ${x.instance} (${x.count})`});
   n.addEventListener('mousedown',async()=>{selInst=x;if(await run())draw()});il.append(n)}
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},...HCOLS.map(([,h])=>el('th',{text:T(h)})))));
  const tb=el('tbody');for(const r of [...res.records].reverse()){const tr=el('tr',{class:(r.verified?'':'bad')+(selRec===r.id?' sel':'')},...HCOLS.map(([k])=>el('td',{text:cell(r,k)})));
   tr.addEventListener('mousedown',()=>{selRec=r.id;for(const x of tb.children)x.classList.toggle('sel',x===tr);det.replaceChildren(...details(r))});tb.append(tr)}t.append(tb);
  const det=el('div',{class:'ahdetails'});const cur=res.records.find(r=>r.id===selRec);det.append(...details(cur));
  b.append(filt,el('div',{class:'ahmain'},il,el('div',{class:'grid ahgrid'},t)),el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scRecordDetails')}),det),
   el('div',{class:'btnrow'},el('span',{class:'note',text:fmt('scRecordCount',res.records.length,res.total)}),btn(T('scRefresh'),async()=>{if(await run())draw()}),
    btn(T('scExport'),()=>download('Audit History.csv','﻿'+csv(),'text/csv')),btn(T('scPrintPreview'),printView)))};
 const details=r=>r?[el('div',{},el('b',{text:T('scOldValue')+': '}),document.createTextNode(r.old||'')),el('div',{},el('b',{text:T('scNewValue')+': '}),document.createTextNode(r.new||'')),
  el('div',{},el('b',{text:T('scReason')+': '}),document.createTextNode(r.reason||'')),el('div',{class:'note',text:`#${r.id} · ${r.hash.slice(0,16)}…`})]:[el('div',{class:'note',text:T('scSelectRecord')})];
 draw();return d}
async function auditCount(){try{const r=await api('count');message(fmt('scCountText',r.count),T('scAuditRecordCount'))}catch(e){message(e.denied?T('scNoRights'):e.message)}}
async function auditBackup(){try{const r=await api('backup');download(`Audit Records ${new Date().toISOString().slice(0,10)}.json`,r.text,'application/json')}catch(e){message(e.denied?T('scNoRights'):e.message)}}
async function auditRestore(){const text=await pickFile('.json');if(!text)return;try{const r=await api('restore',{text});message(fmt('scRestored',r.restored,r.unverified))}catch(e){message(e.denied?T('scNoRights'):e.message)}}
function auditDelete(){
 dialog(T('scDeleteRecords'),b=>{b.append(field(T('scStartDate'),el('input',{type:'date',id:'adFrom'})),field(T('scEndDate'),el('input',{type:'date',id:'adTo'})),el('p',{class:'note',text:T('scInclusive')}),
   check(T('scDeleteAll'),false,{id:'adAll'}))},
  [[T('ok'),async b=>{const all=b.querySelector('#adAll').checked,f=b.querySelector('#adFrom').value,t=b.querySelector('#adTo').value;
    if(!all&&(!f||!t)){message(T('scDatesEmpty'));return false}if(!all&&t<f){message(T('scEndBeforeStart'));return false}
    if(!(await confirmBox(all?T('scDeleteAllConfirm'):fmt('scDeleteRangeConfirm',f,t))))return false;
    try{const r=await api('delete_records',all?{}:{from:f,to:t});message(fmt('scDeleted',r.deleted));return true}catch(e){message(e.denied?T('scNoRights'):e.message);return false}},true],[T('cancel'),()=>true]])}

// ---------- ESig Administration ----------
async function esigAdmin(){
 if(!can('ESig/ESig Administration Tool/May run application'))return denied();
 const groups=JSON.parse(JSON.stringify(S.esig||[]));let cur=groups[0]?.name;
 const d=dialog(T('scEsigAdmin'),b=>b.classList.add('esigdlg'),[[T('ok'),async()=>{try{await api('save_esig',{groups});await core.refresh();return true}catch(e){message(e.denied?T('scNoRights'):e.message);return false}},true],[T('cancel'),()=>true]]);
 const draw=()=>{const b=d.body;b.replaceChildren();
  const t=el('table',{class:'g'});t.append(el('thead',{},el('tr',{},el('th',{text:T('scActionGroup')}),el('th',{text:T('scEsigRequired')}),el('th',{text:T('scChallenge')}))));
  const tb=el('tbody');for(const g of groups){const c=el('input',{type:'checkbox','data-group':g.name});c.checked=g.required;c.addEventListener('change',()=>{g.required=c.checked});
   const tr=el('tr',{class:g.name===cur?'sel':''},el('td',{text:g.name}),el('td',{class:'c'},c),el('td',{text:g.challenge}));tr.addEventListener('mousedown',e=>{if(e.target!==c){cur=g.name;draw()}});tb.append(tr)}t.append(tb);
  const g=groups.find(x=>x.name===cur);const at=el('table',{class:'g'});at.append(el('thead',{},el('tr',{},el('th',{text:T('scEsigAction')}),el('th',{text:T('scMeaning')}))));
  const atb=el('tbody');for(const a of g?.actions||[]){const i=el('input',{type:'text',value:a.meaning,maxlength:'255'});i.addEventListener('input',()=>{a.meaning=i.value});atb.append(el('tr',{},el('td',{text:a.name}),el('td',{},i)))}at.append(atb);
  const ch=el('textarea',{id:'esChallenge',rows:'3',maxlength:'1024'});ch.value=g?.challenge||'';ch.addEventListener('input',()=>{if(g)g.challenge=ch.value});
  b.append(el('div',{class:'fieldset'},el('div',{class:'legend',text:T('scActionGroups')}),el('div',{class:'grid'},t)),
   el('div',{class:'fieldset'},el('div',{class:'legend',text:fmt('scActionList',cur||'')}),el('div',{class:'grid'},at),field(T('scChallengeText'),ch)))};
 draw();return d}

window.Sec={core,init,can,need,denied,auditState,change,record,esign,reasonDialog,loginDialog,logout,changePassword,securityManager,auditSettings,auditHistory,
 auditCount,auditBackup,auditRestore,auditDelete,esigAdmin,ticket:getTicket,objectCan,objectRights,items,chooseGroup,defaultGroup,groupNames,DEFAULT_GROUP,securityGroups:()=>(S.security_groups&&S.security_groups.length?S.security_groups:['GeneMapper ID-X Security Group']),get user(){return S.user},get state(){return S},refresh:()=>core.refresh()};
})();
