#!/usr/bin/env python3
"""Relate Mojmap API roles to MCP stable members on one identical vanilla JAR.
The resulting names adapt APIs; no block/material/lighting values are copied.
"""
import argparse, json, pathlib, re, zipfile
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--mojmap',type=pathlib.Path,required=True)
p.add_argument('--mcp',type=pathlib.Path,required=True)
p.add_argument('--output',type=pathlib.Path,required=True)
a=p.parse_args()
classes={};reverse={};moj=[];owner=None
for line in a.mojmap.read_text().splitlines():
 if not line or line.startswith('#') or ' -> ' not in line:continue
 if not line[0].isspace():
  owner,obf=line.split(' -> ');obf=obf.rstrip(':');classes[owner]=obf;reverse[obf]=owner
 else:moj.append((owner,*line.strip().split(' -> ')))
with zipfile.ZipFile(a.mcp) as z:tsrg=z.read('config/joined.tsrg').decode().splitlines()
mcpclasses={};mcpfields={};mcpmethods={};owner=None
for line in tsrg:
 if not line or line.startswith('#'):continue
 c=line.strip().split()
 if not line[0].isspace():owner=c[0];mcpclasses[owner]=c[1].replace('/','.')
 elif len(c)==2:mcpfields[(owner,c[0])]=c[1]
 elif len(c)==3:mcpmethods[(owner,c[0],c[1])]=c[2]
def descriptor(name):
 if name.endswith('[]'):return '['+descriptor(name[:-2])
 primitives={'void':'V','boolean':'Z','byte':'B','char':'C','short':'S','int':'I','long':'J','float':'F','double':'D'}
 return primitives.get(name,'L'+classes.get(name,name).replace('.','/')+';')
out={'schema':'nova.oracle-api-role-bridge.v1','classes':[],'fields':[],'methods':[]}
for modern,obf in classes.items():
 if obf in mcpclasses:out['classes'].append([modern,mcpclasses[obf]])
for modern,left,runtime in moj:
 actualOwner=mcpclasses.get(classes[modern])
 if actualOwner is None:continue
 left=re.sub(r'^\d+:\d+:','',left)
 if '(' not in left:
  name=left.rsplit(' ',1)[1];actual=mcpfields.get((classes[modern],runtime))
  if actual:out['fields'].append([modern,name,actualOwner,actual])
 else:
  m=re.match(r'(.+?) ([^ ]+)\((.*?)\)',left)
  if not m or '.' in m[2] or m[2] in ['<init>','<clinit>']:continue
  desc='('+''.join(descriptor(x) for x in m[3].split(',') if x)+')'+descriptor(m[1])
  actual=mcpmethods.get((classes[modern],runtime,desc))
  if actual:out['methods'].append([modern,m[2],actualOwner,actual])
a.output.parent.mkdir(parents=True,exist_ok=True)
a.output.write_text(json.dumps(out,separators=(',',':'))+'\n',encoding='utf-8')
print(json.dumps({k:len(v) for k,v in out.items() if isinstance(v,list)}))
