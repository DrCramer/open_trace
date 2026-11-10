// node ts_web_pw.cjs <tables.js> <out.json> <xmls.json> <cases.json>
const fs=require('fs');const {chromium}=require('playwright');
(async()=>{const b=await chromium.launch();const p=await b.newPage();await p.route('http://ts.test/',r=>r.fulfill({body:'<html></html>',contentType:'text/html'}));await p.goto('http://ts.test/');
const src=fs.readFileSync(process.argv[2],'utf8');const xmls=JSON.parse(fs.readFileSync(process.argv[4],'utf8'));const cases=JSON.parse(fs.readFileSync(process.argv[5],'utf8'));
const out=await p.evaluate(([src,xmls,cases])=>{window.T=k=>k;eval(src);const TS=window.TableSettings;const o={};
 o.default=TS.validate(TS.factory());o.defaultXml=TS.toXml(o.default);o.catalog=TS.CATALOG;o.specs={};
 for(const kind of ['samples','genotypes']){o.specs[kind]={};for(const c of TS.CATALOG[kind])o.specs[kind][c.id]=TS.filterSpec(kind,c.id)}
 o.cases=cases.map(c=>{try{const s=TS.validate(c);return {ok:s,xml:TS.toXml(s)}}catch(e){return {err:String(e.message)}}});
 o.imports=xmls.map(x=>{try{const s=TS.fromXml(x);return {ok:s,xml:TS.toXml(s)}}catch(e){return {err:String(e.message)}}});
 const M=[['samples','SQ',{op:3},'low'],['samples','SQ',{op:3},'pass'],['samples','SQ',{op:9},'pass'],['samples','sampleName',{op:1,text:'ЛеС'},'Лестница 1'],['samples','sampleName',{op:1,text:'zz'},'Лестница 1'],
  ['genotypes','dye',{op:2},'B'],['genotypes','dye',{op:2},'G'],['genotypes','GQ',{op:0},'low'],['genotypes','PLI',{op:2},'na'],['genotypes','PLI',{op:1},'na'],['samples','status',{op:3},'changed'],['samples','sampleFileNorm',{op:3},'na'],
  ['samples','specimenCategory',{op:1},'No Export'],['samples','SFNF',{op:2},'low'],['genotypes','ADO',{op:1},'check'],['genotypes','allele',{op:1,text:'ol'},'12 OL']];
 o.matches=M.map(([kind,id,f,raw])=>({kind,id,f,raw,out:TS.matches(kind,id,f,raw)}));
 return o},[src,xmls,cases]);
fs.writeFileSync(process.argv[3],JSON.stringify(out));await b.close()})();
