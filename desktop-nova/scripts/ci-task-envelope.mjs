import fs from 'node:fs';
import crypto from 'node:crypto';
import { pipeline } from 'node:stream/promises';

const magic = Buffer.from('NOVA-CI-AEAD1\n');
function taskKey(context, text) {
  const master = Buffer.from(text || '', 'base64');
  if (master.length !== 32 || master.toString('base64') !== text) throw new Error('Missing task key.');
  try { return Buffer.from(crypto.hkdfSync('sha256', master, Buffer.from(context.taskId), Buffer.from(JSON.stringify(context)), 32)); }
  finally { master.fill(0); }
}
function validate(context) {
  if (context.schema !== 'nova.ci.task.v1' || !/^[0-9]{1,30}$/.test(context.taskId) || !/^[a-f0-9]{40}$/.test(context.sourceSha) ||
      !['x86_64-apple-darwin','aarch64-apple-darwin'].includes(context.target) || !['source','result'].includes(context.direction) || Object.keys(context).length !== 5) throw new Error('Invalid task binding.');
}
async function writeExclusive(output, write) {
  // An EEXIST here grants no ownership and must never enter cleanup.
  const fd = fs.openSync(output, 'wx', 0o600);
  const destination = fs.createWriteStream(output, { fd });
  const closed = new Promise(resolve => destination.once('close', resolve));
  try { await write(destination); }
  catch (error) {
    destination.destroy();
    await closed;
    try { fs.unlinkSync(output); } catch {}
    throw error;
  }
}
export async function sealTask(input, output, context, keyText = process.env.NOVA_CI_TASK_KEY) {
  validate(context);
  const nonce = crypto.randomBytes(12), header = Buffer.from(JSON.stringify({ ...context, nonce: nonce.toString('base64') }));
  const length = Buffer.alloc(4); length.writeUInt32BE(header.length);
  const aad = Buffer.concat([magic, length, header]), key = taskKey(context, keyText);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce); cipher.setAAD(aad);
  try {
    await writeExclusive(output, destination => pipeline(fs.createReadStream(input), cipher, async function* (encrypted) {
      yield aad;
      yield* encrypted;
      yield cipher.getAuthTag();
    }, destination));
  }
  finally { key.fill(0); }
}
export async function openTask(input, output, expected, keyText = process.env.NOVA_CI_TASK_KEY) {
  validate(expected);
  const descriptor = fs.openSync(input, 'r'); let aad, nonce, tag, start, end;
  try {
    const prefix = Buffer.alloc(magic.length + 4); fs.readSync(descriptor, prefix, 0, prefix.length, 0);
    if (!prefix.subarray(0, magic.length).equals(magic)) throw new Error('Invalid task envelope.');
    const count = prefix.readUInt32BE(magic.length); if (count < 1 || count > 4096) throw new Error('Invalid task header.');
    const header = Buffer.alloc(count); fs.readSync(descriptor, header, 0, count, prefix.length);
    const parsed = JSON.parse(header.toString('utf8')); nonce = Buffer.from(parsed.nonce || '', 'base64');
    const actual = Object.fromEntries(Object.keys(expected).map((key) => [key, parsed[key]]));
    if (JSON.stringify(actual) !== JSON.stringify(expected) || Object.keys(parsed).length !== 6 || nonce.length !== 12) throw new Error('Task identity differs.');
    aad = Buffer.concat([prefix, header]); start = aad.length; const size = fs.fstatSync(descriptor).size;
    if (size <= start + 16 || size > 2 * 1024 ** 3) throw new Error('Invalid task envelope size.');
    end = size - 17; tag = Buffer.alloc(16); fs.readSync(descriptor, tag, 0, 16, size - 16);
  } finally { fs.closeSync(descriptor); }
  const key = taskKey(expected, keyText), decipher = crypto.createDecipheriv('aes-256-gcm', key, nonce);
  decipher.setAAD(aad); decipher.setAuthTag(tag);
  try { await writeExclusive(output, destination => pipeline(fs.createReadStream(input, { start, end }), decipher, destination)); }
  finally { key.fill(0); }
}
