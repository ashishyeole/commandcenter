#!/usr/bin/env python3
"""Prepare an unpublished Tauri update manifest from release artifacts and signatures.
Does not upload, sign, execute, or install anything. Refuses ambiguous/missing files.
"""
import argparse, datetime, hashlib, json, re
from pathlib import Path
from urllib.parse import urlparse, quote

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('--version',required=True)
parser.add_argument('--base-url',required=True,help='HTTPS directory where the reviewed files will be published')
parser.add_argument('--artifact',action='append',required=True,metavar='PLATFORM=PATH',help='darwin-aarch64, darwin-x86_64 or windows-x86_64; repeat for each platform')
parser.add_argument('--notes-file',required=True,type=Path)
parser.add_argument('--output',required=True,type=Path)
args=parser.parse_args()
if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?',args.version): parser.error('Invalid version')
u=urlparse(args.base_url)
if u.scheme!='https' or not u.hostname or u.username or u.password or u.query or u.fragment: parser.error('Use an HTTPS directory without credentials, query or fragment')
notes=args.notes_file.read_text()
if len(notes.encode())>64000: parser.error('Release notes exceed 64 KB')
platforms={}; inventory=[];names=set()
for spec in args.artifact:
    platform,sep,filename=spec.partition('=')
    if not sep or platform not in ('darwin-aarch64','darwin-x86_64','windows-x86_64') or platform in platforms: parser.error('Invalid or repeated platform')
    path=Path(filename)
    if not path.is_file() or path.is_symlink() or path.stat().st_size==0: parser.error('Artifact must be a nonempty regular file')
    if path.name in names: parser.error('Artifact filenames must be unique across platforms')
    names.add(path.name)
    if platform.startswith('darwin-') and not path.name.endswith('.app.tar.gz'): parser.error('macOS updates require the Tauri .app.tar.gz updater artifact')
    if platform.startswith('windows-') and not path.name.endswith('.exe'): parser.error('Windows updates require the NSIS .exe artifact')
    signature=Path(str(path)+'.sig')
    if not signature.is_file() or signature.stat().st_size>16000: parser.error('Missing or oversized .sig beside artifact')
    sig=signature.read_text().strip()
    if not sig: parser.error('Empty signature')
    with path.open('rb') as artifact_stream: digest=hashlib.file_digest(artifact_stream,'sha256').hexdigest()
    platforms[platform]={'signature':sig,'url':args.base_url.rstrip('/')+'/'+quote(path.name,safe='')}
    inventory.append({'platform':platform,'file':str(path),'bytes':path.stat().st_size,'sha256':digest,'signature_file':str(signature)})
manifest={'version':args.version,'notes':notes,'pub_date':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platforms':platforms}
if args.output.exists() or Path(str(args.output)+'.inventory.json').exists(): parser.error('Output manifest or inventory already exists')
# Exclusive create: never silently replace an already reviewed release manifest.
with args.output.open('x') as stream: json.dump(manifest,stream,indent=2);stream.write('\n')
with Path(str(args.output)+'.inventory.json').open('x') as stream: json.dump({'published':False,'signature_verified_by_this_script':False,'artifacts':inventory},stream,indent=2);stream.write('\n')
print(f'Prepared {args.output}. Not published. Verify publisher signatures and OS signing before release.')
