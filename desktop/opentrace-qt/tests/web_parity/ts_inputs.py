# Inputs of the Table Settings web-parity test: the shipped ID-X table settings plus hostile XML (ts_xmls.json) and
# settings for validate() (ts_cases.json). Usage: python3 ts_inputs.py <repo root> <out dir>
import copy, glob, json, sys
root, out = sys.argv[1], sys.argv[2]
xmls = [open(f, encoding='utf-8').read().replace('\r\n', '\n') for f in sorted(glob.glob(root + '/resources/data/shipped_table_settings/*.xml'))]
xmls += ['<a/>', '<!DOCTYPE x><TableSettingContainer/>', 'not xml',
 '<TableSettingContainer><xmlProjectTableProfile><profileName>  </profileName></xmlProjectTableProfile></TableSettingContainer>',
 '<TableSettingContainer><xmlProjectTableProfile><profileName> Только имя &amp; &lt;x&gt; </profileName></xmlProjectTableProfile></TableSettingContainer>',
 '<TableSettingContainer><xmlProjectTableProfile><markerProfile><alleleCol><visible> true </visible><filterCriteria><operation>1</operation><operand1> OL </operand1></filterCriteria></alleleCol><dyeColorCol><visible>true</visible><filterCriteria><operation>3</operation><operand1>x</operand1></filterCriteria></dyeColorCol><alleleConfidence><visible>true</visible><filterCriteria><operation>7</operation></filterCriteria></alleleConfidence><numAllelesToShow>40</numAllelesToShow></markerProfile><sampleColsSort>Sample Name</sampleColsSort><sampleColsSort>None</sampleColsSort><sampleColsSort>SQ</sampleColsSort><sampleSortColOrder>1</sampleSortColOrder><sampleSortColOrder>0</sampleSortColOrder><sampleSortColOrder>1</sampleSortColOrder><markerColsSort>Dye</markerColsSort><markerSortOrder>0</markerSortOrder><sampleProfile><sizeStdQualityCol><visible>true</visible><filterCriteria><operation>4</operation><operand1></operand1></filterCriteria></sizeStdQualityCol><sampleNameCol><visible>false</visible><filterCriteria><operation>5</operation><operand1>abc</operand1></filterCriteria></sampleNameCol><statusCol><visible>true</visible></statusCol></sampleProfile></xmlProjectTableProfile></TableSettingContainer>',
 '<TableSettingContainer><xmlProjectTableProfile><profileName>n0</profileName><markerProfile><numAllelesToShow>0</numAllelesToShow></markerProfile></xmlProjectTableProfile></TableSettingContainer>',
 '<TableSettingContainer><xmlProjectTableProfile><profileName>n-3</profileName><markerProfile><numAllelesToShow>-3</numAllelesToShow></markerProfile></xmlProjectTableProfile></TableSettingContainer>',
 '<x:TableSettingContainer xmlns:x="urn:a"><xmlProjectTableProfile><profileName>ns</profileName></xmlProjectTableProfile></x:TableSettingContainer>']
json.dump(xmls, open(out + '/ts_xmls.json', 'w'), ensure_ascii=False)
base = {"name": "Data Interpretation", "samples": {"visible": ["status", "sampleFile", "SQ"], "sort": [{"col": "sampleFile", "desc": False}]},
        "genotypes": {"visible": ["sampleName", "marker", "allele"], "numAlleles": 2, "sort": []}}
def v(**k):
    s = copy.deepcopy(base)
    for path, val in k.items():
        o = s; ks = path.split('__')
        for a in ks[:-1]: o = o[a]
        o[ks[-1]] = val
    return s
cases = [base,
 v(name=" Тест & <1> "), v(name=""), v(name="   "), v(name=5), v(name="x" * 241), v(name="я" * 240),
 v(samples__visible=["status", "nope", "SQ", "SQ", 5, None]),
 v(samples__sort=[{"col": "sampleFile"}, {"col": "x"}, None, {"col": "SQ", "desc": 1}, {"col": "well", "desc": "yes"}, {"col": "panel"}]),
 v(samples__filters={"SQ": {"op": 3}, "SOS": {"op": 4}, "sampleName": {"op": 2, "text": "ab"}, "well": {"op": 1, "text": ""}, "zzz": {"op": 1}, "CGQ": {"op": "4"}, "MIX": {"op": 2.9},
                     "sampleFile": {"op": 1, "text": 123}, "panel": None, "runName": {"op": 1, "text": "й" * 250}, "SE": {"op": 0}, "status": {"op": 3, "text": "junk"}, "comments": {"text": "no op"}}),
 v(genotypes__filters={"dye": {"op": 6}, "GQ": {"op": 4}, "PLI": {"op": 2}, "allele": {"op": 1, "text": "OL"}, "OS": {"op": 4}, "ME": {"op": 1}, "profileId": {"op": 1, "text": "p"}}),
 v(genotypes__numAlleles="4"), v(genotypes__numAlleles=0), v(genotypes__numAlleles=21), v(genotypes__numAlleles=2.5), v(genotypes__numAlleles="x"), v(genotypes__numAlleles=None), v(genotypes__numAlleles=20),
 v(genotypes__limitToBins=True, genotypes__onlyBinned=1), v(genotypes__fontSize=11.5, samples__fontSize="12"), v(genotypes__fontSize=7, samples__fontSize=25), v(samples__fontSize=24, genotypes__fontSize=8),
 v(extra={"keep": 1}, samples__junk=[1]),
 {"name": "no tables"}, {"name": "bad", "samples": {"visible": "x"}, "genotypes": {"visible": [], "numAlleles": 2}}]
c2 = copy.deepcopy(base); del c2["genotypes"]["numAlleles"]; cases.append(c2)
json.dump(cases, open(out + '/ts_cases.json', 'w'), ensure_ascii=False)
