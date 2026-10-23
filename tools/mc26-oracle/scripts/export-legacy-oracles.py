#!/usr/bin/env python3
"""Export pre-flattening vanilla queries using exact client bytecode and MCP maps."""
import argparse
import csv
import hashlib
import io
import json
import pathlib
import re
import shutil
import subprocess
import uuid
import zipfile

CSV = {'1.8':'18-1.8', '1.9':'24-1.9', '1.9.2':'24-1.9', '1.9.4':'26-1.9.4',
       '1.10':'29-1.10.2', '1.10.1':'29-1.10.2', '1.10.2':'29-1.10.2',
       '1.11':'32-1.11', '1.11.2':'32-1.11', '1.12':'39-1.12', '1.12.1':'39-1.12', '1.12.2':'39-1.12'}
ROOT = pathlib.Path(__file__).resolve().parents[3]
SOURCE = ROOT/'tools/mc26-oracle/src/main/java/net/litematicaba/mc26oracle'


def actual_protocol(runtime, srg, names_zip, javap):
    with zipfile.ZipFile(srg) as source:
        records = [row.split() for row in source.read('joined.srg').decode().splitlines()]
    with zipfile.ZipFile(names_zip) as source:
        names = {row['searge']:row['name'] for row in csv.DictReader(io.StringIO(source.read('methods.csv').decode()))}
    classes = {row[2]:row[1] for row in records if row[0]=='CL:'}
    owner = classes['net/minecraft/server/network/NetHandlerHandshakeTCP']
    getters = [row[1].replace('/','.') for row in records if row[0]=='MD:'
               and row[3].startswith('net/minecraft/network/handshake/client/C00Handshake/')
               and names.get(row[3].rsplit('/',1)[1])=='getProtocolVersion' and row[2]=='()I']
    if len(getters)!=1: raise ValueError('exact target handshake protocol accessor is missing or ambiguous')
    bytecode = subprocess.check_output([javap,'-p','-c','-classpath',runtime['client_jar'],owner], text=True)
    instructions = [line.strip() for line in bytecode.splitlines() if re.match(r'\s*\d+:',line)]
    guards = {}
    for index,line in enumerate(instructions[:-2]):
        if '// Method '+getters[0]+':()I' not in line: continue
        constant = re.fullmatch(r'\d+:\s+(?:bipush|sipush)\s+(-?\d+)',instructions[index+1])
        branch = re.match(r'\d+:\s+(if_icmp\w+)\s+',instructions[index+2])
        if constant and branch: guards[branch[1]]=int(constant[1])
    if not {'if_icmple','if_icmpge'} <= guards.keys() or guards['if_icmple']!=guards['if_icmpge']:
        raise ValueError('actual handshake bytecode does not contain matching upper/lower protocol guards')
    return guards['if_icmple']


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('versions',nargs='+',choices=CSV)
    parser.add_argument('--cache',type=pathlib.Path,default=ROOT/'.tmp/game-data/version-runtime')
    parser.add_argument('--output',type=pathlib.Path,default=ROOT/'.tmp/game-data/oracle-exports')
    parser.add_argument('--classes',type=pathlib.Path,default=ROOT/'.tmp/game-data/legacy-oracle-classes')
    parser.add_argument('--java',default='java')
    parser.add_argument('--javac',default='javac')
    parser.add_argument('--javap',default='javap')
    options=parser.parse_args()
    cache=options.cache.resolve();output=options.output.resolve();classes=options.classes.resolve()
    classes.mkdir(parents=True,exist_ok=True);output.mkdir(parents=True,exist_ok=True)
    sources=[SOURCE/name for name in ['MappingAccess.java','LegacyOracleMain.java','LegacyWorldAccess.java']]
    source_id=hashlib.sha256(b''.join(p.read_bytes() for p in sources)).hexdigest()
    first=json.loads((cache/options.versions[0]/'runtime.json').read_bytes())
    subprocess.run([options.javac,'--release','8','-cp',first['classpath'],'-d',str(classes),*[str(p)for p in sources]],check=True)
    failures=[]
    for version in dict.fromkeys(options.versions):
        try:
            runtime=json.loads((cache/version/'runtime.json').read_bytes())
            if runtime['minecraft_version']!=version:raise ValueError('exact runtime version differs')
            # 1.10.1 has no separately published MCP SRG. The project's actual
            # class/signature comparison verifies its 1.10 symbol map; all data
            # queries below still execute the exact 1.10.1 client bytecode.
            srg=cache/'mcp'/f'{"1.10" if version=="1.10.1" else version}-srg.zip';csv_zip=cache/'mcp'/f'{CSV[version]}-csv.zip'
            protocol=actual_protocol(runtime,srg,csv_zip,options.javap)
            input_id=hashlib.sha256(json.dumps([source_id,version,runtime['client_sha256'],protocol,
                hashlib.sha256(srg.read_bytes()).hexdigest(),hashlib.sha256(csv_zip.read_bytes()).hexdigest()],sort_keys=True).encode()).hexdigest()
            destination=output/version;marker=output/(version+'.producer.json')
            if destination.exists() and marker.exists() and json.loads(marker.read_bytes()).get('input_id')==input_id:
                print(json.dumps({'minecraft':version,'status':'reused-exact-legacy-export'}),flush=True);continue
            if destination.exists():
                manifest=json.loads((destination/'oracle-manifest.json').read_bytes())
                if manifest['minecraft']!=version or manifest['client_jar_sha256']!=runtime['client_sha256']:
                    raise ValueError('refusing to replace a different export identity')
            staging=output/('.'+version+'-next-'+uuid.uuid4().hex)
            command=[options.java,'-Xmx2g','-Djava.awt.headless=true','-Dfile.encoding=UTF-8','-cp',str(classes)+';'+runtime['classpath'],
                'net.litematicaba.mc26oracle.LegacyOracleMain','--minecraft',version,'--client-jar',runtime['client_jar'],
                '--client-sha256',runtime['client_sha256'],'--protocol-version',str(protocol),'--mcp-srg',str(srg),'--mcp-csv',str(csv_zip),
                '--legacy-registry',str(cache/'mcp/legacy-supplement.json'),'--generated-world-dir',str(cache/version/'oracle-world'),
                '--output',str(staging)]
            with (output/(version+'-legacy.log')).open('wb')as log:result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
            if result.returncode:
                tail=(output/(version+'-legacy.log')).read_text(encoding='utf-8',errors='replace').splitlines()[-16:]
                raise RuntimeError('\n'.join(tail))
            manifest=json.loads((staging/'oracle-manifest.json').read_bytes())
            if manifest['minecraft']!=version or manifest['client_jar_sha256']!=runtime['client_sha256'] or manifest['protocol_version']!=protocol:
                raise ValueError('completed legacy export identity differs from its actual inputs')
            if destination.exists():
                if destination.parent!=output or destination.is_symlink():raise ValueError('invalid generated export replacement path')
                shutil.rmtree(destination)
            staging.rename(destination);marker.write_text(json.dumps({'input_id':input_id})+'\n',encoding='utf-8')
            print(json.dumps({'minecraft':version,'status':'exported-exact-legacy','states':manifest['block_state_semantics']['state_count'],'protocol_version':protocol}),flush=True)
        except Exception as error:
            failures.append(version);print(json.dumps({'minecraft':version,'status':'failed','error':str(error)},ensure_ascii=False),flush=True)
    if failures:raise SystemExit(1)


if __name__=='__main__':main()
