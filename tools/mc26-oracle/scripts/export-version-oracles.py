#!/usr/bin/env python3
"""Run exact-version vanilla oracle extraction, without a client or renderer."""
import argparse, json, pathlib, subprocess, sys
if hasattr(sys.stdout, "reconfigure"): sys.stdout.reconfigure(encoding="utf-8", errors="replace")

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('versions', nargs='+')
p.add_argument('--cache', type=pathlib.Path, default=pathlib.Path('.tmp/game-data/version-runtime'))
p.add_argument('--output', type=pathlib.Path, default=pathlib.Path('.tmp/game-data/oracle-exports'))
p.add_argument('--classes', type=pathlib.Path, default=pathlib.Path('.tmp/version-oracle/classes'))
p.add_argument('--java', default='C:/Program Files/Eclipse Adoptium/jdk-25.0.3.9-hotspot/bin/java.exe')
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
failures = []
for version in dict.fromkeys(a.versions):
    try:
        descriptor = json.loads((a.cache/version/'runtime.json').read_bytes())
        if descriptor['minecraft_version'] != version: raise ValueError('runtime version mismatch')
        destination = a.output/version
        if destination.exists():
            identity = json.loads((destination/'oracle-manifest.json').read_bytes())
            if identity['minecraft'] != version or identity['client_jar_sha256'] != descriptor['client_sha256']:
                raise ValueError('existing export has a different input identity')
            print(json.dumps({'version':version,'status':'reused-exact-export'}),flush=True)
            continue
        command = [a.java, '-Xmx4G', '-Djava.awt.headless=true', '-Dfile.encoding=UTF-8']
        if descriptor.get('mappings'): command += ['-Dnova.oracle.mappings='+descriptor['mappings']]
        elif version.startswith(('1.13','1.14')):
            csvs={'1.13':'43','1.13.1':'45','1.13.2':'47','1.14':'49','1.14.1':'51','1.14.2':'53','1.14.3':'56'}
            mcp=a.cache/'mcp'
            if version.startswith('1.13'):
                import hashlib
                registry=mcp/'legacy-supplement.json'
                source=json.loads(registry.read_bytes())
                command += ['-Dnova.oracle.flattened=true',
                            '-Dnova.oracle.worldVersion='+str(source['versions'][version]['data_version']),
                            '-Dnova.oracle.registrySha256='+hashlib.sha256(registry.read_bytes()).hexdigest()]
            command += ['-Dnova.oracle.mcp.srg='+str((mcp/(version+'-config.zip')).resolve()),
                        '-Dnova.oracle.mcp.csv='+str((mcp/(csvs[version]+'-'+version+'-csv.zip')).resolve()),
                        '-Dnova.oracle.mcp.bridge='+str((mcp/'1.14.4-oracle-bridge.json').resolve())]
        command += ['-cp', str(a.classes.resolve())+';'+descriptor['classpath'],
            'net.litematicaba.mc26oracle.HeadlessOracleMain','--output',str(destination),
            '--minecraft',version,'--client-sha256',descriptor['client_sha256'],'--scope','render']
        with (a.output/(version+'.log')).open('wb') as log:
            result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
        if result.returncode:
            lines=(a.output/(version+'.log')).read_text(encoding='utf-8',errors='replace').splitlines()
            raise RuntimeError('\n'.join(lines[-20:]))
        identity=json.loads((destination/'oracle-manifest.json').read_bytes())
        print(json.dumps({'version':version,'status':'exported','states':identity['block_state_semantics']['state_count'], 'artifacts':identity['ready_headless_sets']}),flush=True)
    except Exception as failure:
        failures.append({'version':version,'error':str(failure)})
        print(json.dumps(failures[-1],ensure_ascii=False),flush=True)
if failures: sys.exit(1)
