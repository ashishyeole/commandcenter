#!/usr/bin/env python3
"""Record existing build outputs. Hashing is not runtime or signing verification."""
from pathlib import Path
import datetime, hashlib, json, plistlib
ROOT=Path(__file__).resolve().parents[1]
import subprocess
version=json.loads((ROOT/'package.json').read_text())['version']
desktop_revision=subprocess.check_output(['git','log','-1','--format=%H','--','apps/desktop','crates/core'],cwd=ROOT,text=True).strip()
mobile_revision=subprocess.check_output(['git','log','-1','--format=%H','--','apps/mobile-ios','apps/mobile-android'],cwd=ROOT,text=True).strip()
evidence=ROOT/'docs/testing/windows-package-qa.json'
qa=json.loads(evidence.read_text()) if evidence.exists() else {}
artifacts=[('macOS app',ROOT/'target/debug/bundle/macos/Personal Command Center.app'),('macOS DMG',ROOT/f'target/debug/bundle/dmg/Personal Command Center_{version}_arm64.dmg'),('iOS Simulator app',ROOT/'target/mobile-ios-final/Build/Products/Debug-iphonesimulator/CommandCenter.app'),('Android ARM64 debug APK',ROOT/'apps/mobile-android/app/build/outputs/apk/debug/app-debug.apk')]
def sha(path):
    digest=hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda:stream.read(1024*1024),b''): digest.update(chunk)
    return digest.hexdigest()
windows=ROOT/'target/windows-package/nsis'/f'Personal Command Center_{version}_x64-setup.exe'
if windows.exists(): artifacts.append(('Windows NSIS installer',windows))
rows=[]
for platform,path in artifacts:
    if not path.exists(): raise SystemExit(f'Missing artifact: {path}')
    if path.is_dir():
        plist=path/('Contents/Info.plist' if platform=='macOS app' else 'Info.plist')
        with plist.open('rb') as stream: info=plistlib.load(stream)
        if info['CFBundleShortVersionString']!=version: raise SystemExit(f'Stale package version: {path}')
        entries=[];size=0
        for file in sorted(path.rglob('*'),key=lambda p:p.relative_to(path).as_posix()):
            relative=file.relative_to(path).as_posix()
            if file.is_symlink(): entries.append({'path':relative,'symlink':str(file.readlink())})
            elif file.is_file():
                size+=file.stat().st_size;entries.append({'path':relative,'sha256':sha(file)})
        digest=hashlib.sha256(json.dumps(entries,ensure_ascii=False,separators=(',',':'),sort_keys=True).encode()).hexdigest()
        method='SHA-256 of compact UTF-8 JSON array sorted by relative path; each entry has path plus file sha256 or symlink target; object keys sorted'
        count=len(entries)
    else:
        size=path.stat().st_size;digest=sha(path);method='SHA-256 of file bytes';count=1
    artifact_revision=qa.get('source_revision') if platform=='Windows NSIS installer' else mobile_revision if platform in ['iOS Simulator app','Android ARM64 debug APK'] else desktop_revision
    expected_revision=mobile_revision if platform in ['iOS Simulator app','Android ARM64 debug APK'] else desktop_revision
    rows.append({'platform':platform,'source_revision':artifact_revision,'rebuilt_for_current_functional_changes':artifact_revision==expected_revision and (platform!='Windows NSIS installer' or qa.get('sha256','').lower()==digest),'path':str(path),'bytes':size,'sha256':digest,'hash_method':method,'files':count})
metadata=json.loads((ROOT/'apps/mobile-android/app/build/outputs/apk/debug/output-metadata.json').read_text())
if any(e['versionName']!=version for e in metadata['elements']): raise SystemExit('Stale Android package version')
data={'version':version,'generated_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'runtime_tested':False,'isolated_mac_qa_smoke_tested':True,'production_signed':False,'artifacts':rows,'dependency_locks':{p:sha(ROOT/p) for p in ['Cargo.lock','package-lock.json','deploy/terraform/.terraform.lock.hcl','services/optional-host/requirements-push.lock']},'packaging_notes':['Local Mac development app/DMG and mobile development binaries; Windows release NSIS installer from the identified CI source revision.','Mobile UI binaries were rebuilt for private-file addition/preview; mobile Rust bindings are unchanged. Streaming backup and Google/radar UI are desktop-only.','iOS shared Rust symbols are defined in the bundle; no developer libpcc dylib dependency.','macOS voice bridge is bundled and referenced through @rpath.','Windows release installer passed CI silent install, native-window launch and silent uninstall. A separately identified macOS QA app passed earlier selected native workflows; selected four-theme, section navigation, radar/draft and default-off Google checks also passed in isolated Mac QA. Normal artifacts have not all received feature acceptance. No tool container or hosted service was launched. See docs/testing/results.json for exact coverage.']}
data['desktop_source_revision']=desktop_revision
data['windows_installer_smoke_verified']=windows.exists() and qa.get('install_exit')==0 and qa.get('uninstall_exit')==0 and qa.get('sha256','').lower()==sha(windows)
data['mobile_ui_source_revision']=mobile_revision
data['runtime_coverage_by_platform']={'macOS':'Selected isolated native QA workflows passed, including four themes, theme/launcher persistence, full-color thumbnail, section navigation, research drafts and default-off Google stop/clear. Full feature acceptance remains open.','Windows':'Installer/native-window/uninstaller smoke passed' if data['windows_installer_smoke_verified'] else 'No matching smoke evidence','iOS':'Compilation only','Android':'Compilation only'}
(ROOT/'docs/BUILD-ARTIFACTS.json').write_text(json.dumps(data,indent=2)+'\n')
print('Recorded',version,'and',len(rows),'existing artifacts. No runtime verification performed.')
