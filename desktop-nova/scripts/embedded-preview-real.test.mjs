import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync, writeFileSync, mkdirSync, readdirSync, copyFileSync, existsSync } from "node:fs";
import { resolve, join } from "node:path";
import { deflateSync, inflateSync } from "node:zlib";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";

const repo=resolve(import.meta.dirname,"../..");
const evidence=join(repo,".tmp/beta-ui-corrections/embedded-preview-real");
const core=process.env.NOVA_INTEGRATION_CORE || join(repo,"tools/viewer-core/target/debug/litematica_core.exe");
function crc32(bytes) {
  let value=0xffffffff;
  for(const byte of bytes) { value^=byte;for(let bit=0;bit<8;bit++)value=(value>>>1)^((value&1)?0xedb88320:0); }
  return (value^0xffffffff)>>>0;
}
function chunk(kind,payload) {
  const content=Buffer.concat([Buffer.from(kind),payload]);const header=Buffer.alloc(4),crc=Buffer.alloc(4);
  header.writeUInt32BE(payload.length);crc.writeUInt32BE(crc32(content));return Buffer.concat([header,content,crc]);
}
function testPng() {
  const width=12,height=8,header=Buffer.alloc(13);header.writeUInt32BE(width,0);header.writeUInt32BE(height,4);header[8]=8;header[9]=6;
  const rows=Buffer.alloc(height*(width*4+1));
  for(let y=0;y<height;y++)for(let x=0;x<width;x++)rows.set(x>=2&&x<10?[79,123,211,143]:[255,0,0,255],y*(width*4+1)+1+x*4);
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk("IHDR",header),chunk("IDAT",deflateSync(rows)),chunk("IEND",Buffer.alloc(0))]);
}
function decodeRgbaPng(bytes) {
  assert.deepEqual(bytes.subarray(0,8),Buffer.from([137,80,78,71,13,10,26,10]));
  const parts=[];let width,height;
  for(let offset=8;offset<bytes.length;) {
    const length=bytes.readUInt32BE(offset),kind=bytes.toString("ascii",offset+4,offset+8),data=bytes.subarray(offset+8,offset+8+length);
    if(kind==="IHDR") {width=data.readUInt32BE(0);height=data.readUInt32BE(4);assert.equal(data[8],8);assert.equal(data[9],6);}
    if(kind==="IDAT")parts.push(data);offset+=length+12;
  }
  const packed=inflateSync(Buffer.concat(parts)),stride=width*4,rgba=Buffer.alloc(height*stride);
  const paeth=(a,b,c)=>{const p=a+b-c,pa=Math.abs(p-a),pb=Math.abs(p-b),pc=Math.abs(p-c);return pa<=pb&&pa<=pc?a:pb<=pc?b:c;};
  for(let y=0;y<height;y++) {
    const filter=packed[y*(stride+1)];assert.ok(filter<=4);
    for(let x=0;x<stride;x++) {
      const left=x>=4?rgba[y*stride+x-4]:0,above=y?rgba[(y-1)*stride+x]:0,upperLeft=x>=4&&y?rgba[(y-1)*stride+x-4]:0;
      const prediction=[0,left,above,Math.floor((left+above)/2),paeth(left,above,upperLeft)][filter];
      rgba[y*stride+x]=(packed[y*(stride+1)+1+x]+prediction)&255;
    }
  }
  return {width,height,rgba};
}

test("真实小投影：PreviewImage中心裁切/ARGB导入、PNG读取、另存和清除重读，原件和全部统计保持",()=>{
  mkdirSync(evidence,{recursive:true});
  const sampleDirectory="C:\\Users\\27232\\Documents\\投影测试";
  const source=join(sampleDirectory,"16核刷铁机.litematic");
  assert.ok(readdirSync(sampleDirectory).includes("16核刷铁机.litematic"));
  const original=readFileSync(source),sha256=createHash("sha256").update(original).digest("hex"),tag=Date.now().toString(36);
  const sample=join(evidence,`source-${tag}.litematic`);copyFileSync(source,sample);
  const commands=[];
  const rejected=args=>{
    const result=spawnSync(core,args,{encoding:"utf8",cwd:repo,maxBuffer:16*1024*1024});
    commands.push({command:core,args,exitStatus:result.status,expectedFailure:true});
    assert.equal(result.error,undefined);assert.notEqual(result.status,0,"拒绝写入必须返回真实失败");
  };
  const run=args=>{
    const result=spawnSync(core,args,{encoding:"utf8",cwd:repo,maxBuffer:16*1024*1024});
    commands.push({command:core,args,exitStatus:result.status});
    assert.equal(result.status,0,result.stderr);return JSON.parse(result.stdout);
  };
  const before=run(["stats","--input",sample]);
  const image=join(evidence,`image-${tag}.png`);writeFileSync(image,testPng());
  const importPatch=join(evidence,`import-${tag}.json`);writeFileSync(importPatch,JSON.stringify({preview_image_path:image}));
  const output=join(evidence,`with-preview-${tag}.litematic`);
  copyFileSync(sample,output);
  const entriesBeforeSave=readdirSync(evidence).sort();
  const saved=run(["edit-metadata","--input",output,"--patch",importPatch]);
  assert.equal(saved.output,output);assert.equal(saved.backup_path,null);assert.ok(existsSync(output));
  assert.deepEqual(readdirSync(evidence).sort(),entriesBeforeSave,"原地安全写入不遗留额外bak或临时文件");
  const png=join(evidence,`extracted-${tag}.png`);
  const extracted=run(["projection-preview","--json","--input",output,"--output",png]);
  assert.equal(extracted.found,true);assert.equal(extracted.width,140);assert.equal(extracted.height,140);
  const decoded=decodeRgbaPng(readFileSync(png));
  assert.equal(decoded.width,140);assert.equal(decoded.height,140);
  assert.deepEqual(decoded.rgba.subarray((70*140+70)*4,(70*140+70)*4+4),Buffer.from([79,123,211,143]));
  const invalidPatch=join(evidence,`invalid-${tag}.json`);writeFileSync(invalidPatch,JSON.stringify({preview_image_data:[1,2,3]}));
  const outputBeforeRejectedWrite=readFileSync(output),entriesBeforeRejectedWrite=readdirSync(evidence).sort();
  rejected(["edit-metadata","--input",output,"--patch",invalidPatch]);
  assert.deepEqual(readFileSync(output),outputBeforeRejectedWrite,"验证失败不得部分覆盖现有投影");
  assert.deepEqual(readdirSync(evidence).sort(),entriesBeforeRejectedWrite,"失败必须回收本次临时输出");
  const copy=join(evidence,`metadata-copy-${tag}.litematic`),emptyPatch=join(evidence,`keep-${tag}.json`);writeFileSync(emptyPatch,"{}");
  run(["edit-metadata","--input",output,"--patch",emptyPatch,"--output",copy]);
  const existingCopy=readFileSync(copy),existingSource=readFileSync(output),entriesBeforeNoClobber=readdirSync(evidence).sort();
  rejected(["edit-metadata","--input",output,"--patch",emptyPatch,"--output",copy]);
  assert.deepEqual(readFileSync(copy),existingCopy,"另存目标已存在时不覆盖目标");
  assert.deepEqual(readFileSync(output),existingSource,"另存失败不修改来源");
  assert.deepEqual(readdirSync(evidence).sort(),entriesBeforeNoClobber);
  const keptPng=join(evidence,`copy-extracted-${tag}.png`);
  assert.equal(run(["projection-preview","--json","--input",copy,"--output",keptPng]).found,true);
  assert.deepEqual(decodeRgbaPng(readFileSync(keptPng)).rgba,decoded.rgba);
  const clearPatch=join(evidence,`clear-${tag}.json`),cleared=join(evidence,`cleared-${tag}.litematic`);writeFileSync(clearPatch,JSON.stringify({preview_image_data:[]}));
  run(["edit-metadata","--input",copy,"--patch",clearPatch,"--output",cleared]);
  const absent=run(["projection-preview","--json","--input",cleared,"--output",join(evidence,`absent-${tag}.png`)]);assert.equal(absent.found,false);
  const after=run(["stats","--input",cleared]);
  assert.deepEqual(after,before);
  assert.deepEqual(readFileSync(sample),original);
  assert.equal(createHash("sha256").update(readFileSync(source)).digest("hex"),sha256);
  writeFileSync(join(evidence,"result.json"),JSON.stringify({source,sourceSha256:sha256,originalUnchanged:true,noAdditionalBackup:true,failedInPlaceUnchanged:true,saveAsNoClobber:true,noTemporaryFilesLeft:true,allStatsUnchanged:true,totalBlocks:before.structure_stats.total_non_air_blocks,width:decoded.width,height:decoded.height,argbChannels:[79,123,211,143],output,copy,cleared,commands},null,2));
});
