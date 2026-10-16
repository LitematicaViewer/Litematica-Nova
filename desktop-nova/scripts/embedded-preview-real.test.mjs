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
function testPng(width=140,height=70,pixel=(x)=>x>=20&&x<120?[79,123,211,143]:[255,0,0,255]) {
  const header=Buffer.alloc(13);header.writeUInt32BE(width,0);header.writeUInt32BE(height,4);header[8]=8;header[9]=6;
  const rows=Buffer.alloc(height*(width*4+1));
  for(let y=0;y<height;y++)for(let x=0;x<width;x++)rows.set(pixel(x,y),y*(width*4+1)+1+x*4);
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

test("真实小投影：320×240三模式后处理、内嵌PNG读取、另存和清除重读，原件和全部统计保持",()=>{
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
  assert.equal(extracted.found,true);assert.equal(extracted.width,320);assert.equal(extracted.height,240);
  const decoded=decodeRgbaPng(readFileSync(png));
  assert.equal(decoded.width,320);assert.equal(decoded.height,240);
  const pixel=(x,y)=>decoded.rgba.subarray((y*320+x)*4,(y*320+x)*4+4);
  assert.deepEqual(pixel(160,120),Buffer.from([79,123,211,143]));
  assert.deepEqual(pixel(0,120),Buffer.from([255,0,0,255]),"保留导入原图左边缘");
  assert.deepEqual(pixel(319,120),Buffer.from([255,0,0,255]),"保留导入原图右边缘");
  assert.deepEqual(pixel(160,39),Buffer.from([0,0,0,0]),"导入原图上方透明补边");
  assert.deepEqual(pixel(160,40),Buffer.from([79,123,211,143]));
  assert.deepEqual(pixel(160,199),Buffer.from([79,123,211,143]));
  assert.deepEqual(pixel(160,200),Buffer.from([0,0,0,0]),"导入原图下方透明补边");
  const modeResults=[];
  for(const mode of ["fast_experimental","standard","full"]) {
    const cases=mode==="full"?[["sky",600,300,null]]:[
      ["tower",80,160,[30,20,20,120]],["wide",160,80,[20,30,120,20]],["ordinary",100,100,[10,20,80,60]],
    ];
    for(const [shape,w,h,rect] of cases) {
      const input=join(evidence,`${tag}-${mode}-${shape}.png`),patch=join(evidence,`${tag}-${mode}-${shape}.json`);
      const path=join(evidence,`${tag}-${mode}-${shape}.litematic`),png=join(evidence,`${tag}-${mode}-${shape}-out.png`);
      writeFileSync(input,testPng(w,h,(x,y)=>rect
        ?(x>=rect[0]&&x<rect[0]+rect[2]&&y>=rect[1]&&y<rect[1]+rect[3]?[40,150,70,255]:[255,0,0,0])
        :(x<80||x>=520?[255,0,0,255]:x>=299&&x<=301?[0,200,0,255]:[30,90,170,255])));
      writeFileSync(patch,JSON.stringify({preview_image_path:input,preview_image_mode:mode}));
      run(["edit-metadata","--input",sample,"--patch",patch,"--output",path]);
      const info=run(["projection-preview","--json","--input",path,"--output",png]);
      assert.equal(info.width,320);assert.equal(info.height,240);
      const frame=decodeRgbaPng(readFileSync(png));
      let left=320,top=240,right=0,bottom=0;
      for(let y=0;y<240;y++)for(let x=0;x<320;x++)if(frame.rgba[(y*320+x)*4+3]>0){left=Math.min(left,x);top=Math.min(top,y);right=Math.max(right,x+1);bottom=Math.max(bottom,y+1);}
      if(rect){
        const scale=Math.min(310/rect[2],230/rect[3]);
        assert.equal(right-left,Math.floor(rect[2]*scale+1e-9));assert.equal(bottom-top,Math.floor(rect[3]*scale+1e-9));
        assert.ok(left>=5&&top>=5&&right<=315&&bottom<=235);
        assert.ok(Math.abs((left+right)/2-160)<=0.5&&Math.abs((top+bottom)/2-120)<=0.5);
      }else{
        assert.deepEqual([left,top,right,bottom],[0,0,320,240]);
        assert.deepEqual(frame.rgba.subarray(0,4),Buffer.from([30,90,170,255]),"Full裁掉两侧而非添加透明边缘");
        assert.deepEqual(frame.rgba.subarray((120*320+160)*4,(120*320+160)*4+4),Buffer.from([0,200,0,255]));
        assert.ok(frame.rgba.filter((_,index)=>index%4===3).every(alpha=>alpha===255),"保留天空背景");
      }
      modeResults.push({mode,shape,output:path,png,width:320,height:240,alphaBounds:[left,top,right,bottom]});
    }
  }
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
  writeFileSync(join(evidence,"result.json"),JSON.stringify({source,sourceSha256:sha256,originalUnchanged:true,noAdditionalBackup:true,failedInPlaceUnchanged:true,saveAsNoClobber:true,noTemporaryFilesLeft:true,allStatsUnchanged:true,totalBlocks:before.structure_stats.total_non_air_blocks,width:decoded.width,height:decoded.height,argbChannels:[79,123,211,143],output,copy,cleared,modeResults,commands},null,2));
});
