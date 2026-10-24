"""CODIS CMF 3.2 / 3.3 XML export (GeneMapper ID-X "Export Table for CODIS").

Element and attribute layout follows the Castor descriptors shipped in GMp.jar
(package ExportImportCODISFormatCMF, namespace ``urn:CODISImportFile-schema``):

* CODISImportFile: HEADERVERSION, MESSAGETYPE, DESTINATIONORI, SOURCELAB,
  SUBMITBYUSERID, SUBMITDATETIME, BATCHID, KIT, SPECIMEN*
* SPECIMEN attributes SOURCEID, CASEID, NCIC, ViCAP, PARTIAL; elements
  SPECIMENID, SPECIMENCATEGORY, SPECIMENCOMMENT, LOCUS*
* LOCUS attributes BATCHID, PARTIALLOCUS, KIT; elements LOCUSNAME, READINGBY,
  READINGDATETIME, ALLELE*;  ALLELE: ALLELEVALUE

Marker names, limits and kit names come from the CODISMarkers and
GeneMapper_Codis_Kits.txt configuration files (defaults below are the shipped
ones and can be replaced with the user's own copies).
"""
import re
import time
from xml.sax.saxutils import escape, quoteattr

CODIS_MARKERS_DEFAULT = """**#TOTAL_MARKER_FOR_CMF1.0=64
**#TOTAL_MARKER_FOR_CMF3.x=64
**#YMARKER=32
**#STRMARKER=32
Amelogenin,AMEL
DYF387S1
DYS389 I,B_DYS389I
DYS389 II,B_DYS389II
DYS390,B_DYS390
DYS449
DYS456,B_DYS456
DYS460
DYS518
DYS627
CSF1PO
D10S1248
D12S391
D13S317
D16S539
D18S51
D19S433
D1S1656
D21S11
D22S1045
D2S1338
D2S441
D3S1358
D5S818
D7S820
D8S1179
DYS391
FGA
DYS19,G_DYS19
DYS385,G_DYS385
DYS458,G_DYS458
Penta D,Penta_D
Penta E,Penta_E
DYS437,R_DYS437
DYS438,R_DYS438
DYS448,R_DYS448
YGATAH4,R_Y_GATA_H4
SE33
TH01
TPOX
vWA
DYS391,Y_DYS391
DYS392,Y_DYS392
DYS393,Y_DYS393
DYS439,Y_DYS439
DYS635,Y_DYS635
Yindel,Yindel
DYS481
DYS533
DYS549
DYS570
DYS576
DYS643
"""

CODIS_KITS_DEFAULT = """Yfiler,Yfiler
AmpFLSTR NGM SElect,NGM_SElect
AmpFLSTR NGM,NGM
AmpFlSTR Yfiler Plus,Yfiler_Plus_Panel
MiniFiler,MiniFiler
Profiler Plus,Profiler_Plus_CODIS
Profiler Plus,Profiler_Plus
COfiler,COfiler_CODIS
COfiler,COfiler
GlobalFiler Express,GlobalFiler_Express
GlobalFiler,GlobalFiler_Panel
Identifiler Direct,Identifiler_Direct
Identifiler Plus,Identifiler_Plus
Identifiler,Identifiler_CODIS
Identifiler,Identifiler
SGM Plus,SGM_Plus
"""

# SpecimenCategoryType enumeration (castor types) and the default selectable list (insert_codis.sql).
SPECIMEN_CATEGORIES = (
    'Alleged Father', 'Alleged Mother', 'Arrestee', 'Biological Child', 'Biological Father', 'Biological Mother',
    'Biological Sibling', 'CO Duplicate', 'Convicted Offender', 'Deceased', 'Deduced Missing Person',
    'Deduced Suspect', 'Detainee', 'Elimination, Known', 'Forensic Mixture', 'Forensic Partial',
    'Forensic, Unknown', 'Juvenile', 'Legal', 'Maternal Relative', 'Missing Person', 'Multi-allelic Offender',
    'Other', 'Paternal Relative', 'Population', 'Proficiency', 'Spouse', 'Staff', 'Suspect, Known',
    'Unidentified Person', 'Unreviewed Offender', 'Victim, Known', 'Volunteer')
DEFAULT_SPECIMEN_TYPES = (
    'No Export', 'Alleged Father', 'Alleged Mother', 'Biological Child', 'Biological Father', 'Biological Mother',
    'Biological Sibling', 'Convicted Offender', 'Elimination, Known', 'Forensic Mixture', 'Forensic, Unknown',
    'Maternal Relative', 'Missing Person', 'Other', 'Paternal Relative', 'Population', 'Proficiency',
    'Suspect, Known', 'Unidentified Person', 'Victim, Known', 'Deduced Missing Person', 'Detainee',
    'Forensic Partial', 'Multi-allelic Offender', 'Unreviewed Offender')
VERSIONS = ('3.2', '3.3')
COMMENT_LIMIT = {'3.2': 255, '3.3': 512}
ALLELE = re.compile(r'^[<>]?\d{1,3}(\.\d{1,2})?[<>]?$')
NAME_BAD = re.compile(r'[\\/:*?"<>|]')


def parse_codis_markers(text=None):
    """Return ({panel marker name: CODIS locus name}, limits)."""
    text = CODIS_MARKERS_DEFAULT if text is None else text
    if not isinstance(text, str) or len(text) > 262144:
        raise ValueError('CODISMarkers text required (at most 256 KiB)')
    limits = {'TOTAL_MARKER_FOR_CMF1.0': 64, 'TOTAL_MARKER_FOR_CMF3.x': 64, 'YMARKER': 32, 'STRMARKER': 32}
    mapping, seen = {}, set()
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith('**#'):
            key, _, value = line[3:].partition('=')
            if key.strip() in limits:
                if not value.strip().isdigit():
                    raise ValueError(f'CODISMarkers: {key.strip()} needs a numeric value')
                limits[key.strip()] = int(value.strip())
            continue
        if not line or line.startswith('**'):
            continue
        codis, _, panel = line.partition(',')
        codis, panel = codis.strip(), (panel.strip() or codis.strip())
        if NAME_BAD.search(panel) or ' ' in panel:
            raise ValueError(f'CODISMarkers: invalid panel marker name {panel!r}')
        if panel.lower() in seen:
            raise ValueError(f'CODISMarkers: duplicate marker {panel!r}')
        seen.add(panel.lower())
        mapping[panel] = codis
    return mapping, limits


def parse_codis_kits(text=None):
    """Return [(panel kit prefix, CODIS kit name)] in file order."""
    text = CODIS_KITS_DEFAULT if text is None else text
    out = []
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith('#'):
            continue
        codis, _, panel = line.partition(',')
        out.append(((panel.strip() or codis.strip()), codis.strip()))
    return out


def codis_kit_name(panel, kits):
    """CODIS kit for a panel name: the longest panel-kit prefix without version wins."""
    best = None
    for prefix, codis in kits:
        if panel == prefix or panel.lower().startswith(prefix.lower()):
            if best is None or len(prefix) > len(best[0]):
                best = (prefix, codis)
    return best[1] if best else panel


def is_y_locus(name):
    """Y-chromosome locus by CODIS name (DYS..., DYF..., Y GATA H4, Yindel)."""
    return name.upper().startswith(('DY', 'Y'))


def _text(value, field, limit=255):
    value = '' if value is None else str(value).strip()
    if '|' in value or ';' in value:
        raise ValueError(f'{field}: text must not contain | or ;')
    if len(value) > limit:
        raise ValueError(f'{field}: at most {limit} characters')
    return value


def _bool(value, field):
    if value in (None, ''):
        return None
    if isinstance(value, bool):
        return value
    text = str(value).strip().lower()
    if text in ('true', 't', 'yes', '1'):
        return True
    if text in ('false', 'f', 'no', '0'):
        return False
    raise ValueError(f'{field}: Partial Profile Indicator must be True or False')


def build_cmf(request, *, markers_text=None, kits_text=None):
    """Build a CMF 3.2/3.3 XML import file from explicit sample entries.

    request = {version, source_lab, destination_ori, user, submit_datetime?,
               samples: [{sample_name, specimen_id, category, panel, batch_id,
                          case_id, comment, partial, source_id, ncic, vicap,
                          loci: [{marker, alleles: [..], partial_locus?: bool}]}]}
    """
    if not isinstance(request, dict):
        raise ValueError('CODIS export request must be an object')
    version = str(request.get('version', '3.2'))
    if version not in VERSIONS:
        raise ValueError('CMF version must be 3.2 or 3.3')
    source = _text(request.get('source_lab'), 'Source Lab ID', 64)
    dest = _text(request.get('destination_ori'), 'Destination Lab ID', 64)
    user = _text(request.get('user'), 'User', 64)
    if not source or not dest:
        raise ValueError('Select a Source and a Destination Lab ID')
    if not user:
        raise ValueError('Enter the CODIS user name (it must match the CODIS user)')
    stamp = request.get('submit_datetime') or time.strftime('%Y-%m-%dT%H:%M:%S')
    if not re.match(r'^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d$', str(stamp)):
        raise ValueError('submit_datetime must be YYYY-MM-DDThh:mm:ss')
    mapping, limits = parse_codis_markers(markers_text)
    kits = parse_codis_kits(kits_text)
    samples = request.get('samples')
    if not isinstance(samples, list) or not samples or len(samples) > 5000:
        raise ValueError('Select 1..5000 samples for CODIS export')
    skipped_markers, errors, specimens, order = set(), [], {}, []
    exported = 0
    for entry in samples:
        if not isinstance(entry, dict):
            raise ValueError('Each CODIS sample must be an object')
        name = _text(entry.get('sample_name'), 'Sample Name')
        category = str(entry.get('category') or 'No Export')
        if category == 'No Export':
            continue
        if category not in SPECIMEN_CATEGORIES:
            raise ValueError(f'{name}: {category!r} is not a valid specimen category')
        try:
            sid = _text(entry.get('specimen_id') or name, 'Specimen ID', 64)
            spec = {'category': category, 'case_id': _text(entry.get('case_id'), 'Case ID', 64),
                    'comment': _text(entry.get('comment'), 'Specimen Comment', COMMENT_LIMIT[version]),
                    'partial': _bool(entry.get('partial'), 'Partial Profile Indicator'),
                    'source_id': _text(entry.get('source_id'), 'Source ID', 64),
                    'ncic': _text(entry.get('ncic'), 'NCIC', 64) if version == '3.3' else '',
                    'vicap': _text(entry.get('vicap'), 'ViCAP', 64) if version == '3.3' else ''}
            if spec['ncic'] and len(spec['ncic']) != 10:
                raise ValueError('Specimen NCIC Number (UD7) needs exactly 10 characters')
            if spec['vicap'] and len(spec['vicap']) != 12:
                raise ValueError('Specimen ViCAP Number (UD8) needs exactly 12 characters')
            kit = codis_kit_name(_text(entry.get('panel'), 'Panel', 120), kits)
            batch = _text(entry.get('batch_id'), 'Batch ID', 64)
            loci = []
            for locus in entry.get('loci') or []:
                marker = str(locus.get('marker', ''))
                alleles = [str(a).strip() for a in locus.get('alleles') or [] if str(a).strip()]
                if not alleles:
                    continue
                if marker not in mapping:
                    skipped_markers.add(marker)
                    continue
                codis = mapping[marker]
                for allele in alleles:
                    ok = allele in ('X', 'Y') if codis == 'Amelogenin' else bool(ALLELE.match(allele))
                    if not ok:
                        raise ValueError(f'The allele name {allele} for {marker} is invalid for CODIS export. '
                                         'Assign a valid allele name and try again.')
                loci.append({'name': codis, 'alleles': alleles, 'partial': bool(locus.get('partial_locus')) and version == '3.3',
                             'kit': kit, 'batch': batch})
            if not loci:
                raise ValueError(f'Unable to export {name}: samples designated for CODIS export must contain at least one allele')
        except ValueError as error:
            errors.append(str(error) if name in str(error) else f'{name}: {error}')
            continue
        current = specimens.get(sid)
        if current is None:
            specimens[sid] = {**spec, 'loci': {}, 'samples': [name]}
            order.append(sid)
            current = specimens[sid]
        else:
            labels = {'category': 'specimen categories', 'case_id': 'case identifiers (UD3)', 'comment': 'specimen comments (UD4)',
                      'partial': 'partial profile indicators (UD5)', 'source_id': 'source identifiers (UD6)'}
            for key, label in labels.items():
                if current[key] != spec[key]:
                    errors.append(f'The {label} for {sid} do not match. Assign the same value before exporting to CODIS.')
            current['samples'].append(name)
        for locus in loci:
            prior = current['loci'].get(locus['name'])
            if prior and sorted(prior['alleles']) != sorted(locus['alleles']):
                errors.append(f'Non-concordant genotypes for {sid} at {locus["name"]}: genotypes must be reconciled before exporting to CODIS.')
            elif not prior:
                current['loci'][locus['name']] = locus
        exported += 1
    if errors:
        raise ValueError('\n'.join(dict.fromkeys(errors)))
    if not specimens:
        raise ValueError('All selected samples have specimen type No Export; the export was not performed.')
    for sid in order:
        names = list(specimens[sid]['loci'])
        y = [n for n in names if is_y_locus(n)]
        if len(names) > limits['TOTAL_MARKER_FOR_CMF3.x'] or len(y) > limits['YMARKER'] or len(names) - len(y) > limits['STRMARKER']:
            raise ValueError(f'{sid}: too many loci for CMF ({len(names)}; limits {limits})')
    lines = ['<?xml version="1.0" encoding="UTF-8"?>',
             '<CODISImportFile xmlns="urn:CODISImportFile-schema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">',
             f'<HEADERVERSION>{version}</HEADERVERSION>', '<MESSAGETYPE>Import</MESSAGETYPE>',
             f'<DESTINATIONORI>{escape(dest)}</DESTINATIONORI>', f'<SOURCELAB>{escape(source)}</SOURCELAB>',
             f'<SUBMITBYUSERID>{escape(user)}</SUBMITBYUSERID>', f'<SUBMITDATETIME>{stamp}</SUBMITDATETIME>']
    for sid in order:
        s = specimens[sid]
        attrs = ''
        if s['source_id']:
            attrs += f' SOURCEID={quoteattr(s["source_id"])}'
        if s['case_id']:
            attrs += f' CASEID={quoteattr(s["case_id"])}'
        if s['ncic']:
            attrs += f' NCIC={quoteattr(s["ncic"])}'
        if s['vicap']:
            attrs += f' ViCAP={quoteattr(s["vicap"])}'
        if s['partial'] is not None:
            attrs += f' PARTIAL="{"true" if s["partial"] else "false"}"'
        lines.append(f'<SPECIMEN{attrs}>')
        lines.append(f'<SPECIMENID>{escape(sid)}</SPECIMENID>')
        lines.append(f'<SPECIMENCATEGORY>{escape(s["category"])}</SPECIMENCATEGORY>')
        if s['comment']:
            lines.append(f'<SPECIMENCOMMENT>{escape(s["comment"])}</SPECIMENCOMMENT>')
        for locus in s['loci'].values():
            la = (f' BATCHID={quoteattr(locus["batch"])}' if locus['batch'] else '') + \
                 (' PARTIALLOCUS="true"' if locus['partial'] else '') + f' KIT={quoteattr(locus["kit"])}'
            lines.append(f'<LOCUS{la}>')
            lines.append(f'<LOCUSNAME>{escape(locus["name"])}</LOCUSNAME>')
            lines.append(f'<READINGBY>{escape(user)}</READINGBY>')
            lines.append(f'<READINGDATETIME>{stamp}</READINGDATETIME>')
            for allele in locus['alleles']:
                lines.append(f'<ALLELE><ALLELEVALUE>{escape(allele)}</ALLELEVALUE></ALLELE>')
            lines.append('</LOCUS>')
        lines.append('</SPECIMEN>')
    lines.append('</CODISImportFile>')
    return {'xml': '\n'.join(lines) + '\n', 'version': version, 'specimens': len(order), 'samples': exported,
            'skipped_markers': sorted(m for m in skipped_markers if m)}


def codis_request(request):
    action = request.get('action') if isinstance(request, dict) else None
    if action == 'config':
        mapping, limits = parse_codis_markers(request.get('markers_text'))
        return {'markers': mapping, 'limits': limits, 'kits': parse_codis_kits(request.get('kits_text')),
                'specimen_categories': list(SPECIMEN_CATEGORIES), 'default_specimen_types': list(DEFAULT_SPECIMEN_TYPES)}
    if action == 'export':
        return build_cmf(request, markers_text=request.get('markers_text'), kits_text=request.get('kits_text'))
    raise ValueError('action must be config or export')
