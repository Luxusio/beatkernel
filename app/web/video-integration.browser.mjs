/**
 * Actual MP4 -> dedicated native WebCodecs Worker -> Rust transform ->
 * BrowserView/Renderer/GPU Scene development evidence. No codec/GPU mocks.
 *
 * Generate the fixture (external tools are never installed by this script):
 * BEATKERNEL_TEST_FFMPEG=/path/to/ffmpeg BEATKERNEL_TEST_FFPROBE=/path/to/ffprobe \
 *   node app/web/video-integration.browser.mjs --generate
 * Run after regenerating app/web/pkg with the current production WASM:
 * PUPPETEER_MODULE=/path/to/puppeteer-core CHROMIUM=/path/to/chromium \
 *   node app/web/video-integration.browser.mjs
 * Optional VIDEO_INTEGRATION_OUT, VIDEO_INTEGRATION_PORT, TLS_CERT/TLS_KEY.
 *
 * This fixture commits deterministic original-song preview states through
 * production BKRV exports/imports. It does not claim physical audio cadence,
 * complete live/local/history route coverage, or private GPU texture identity.
 * Independent Harness browser QA must run after actual code review PASS.
 */
import assert from 'node:assert/strict';
import { createServer } from 'node:https';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { X509Certificate, createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { dirname, extname, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const out = resolve(process.env.VIDEO_INTEGRATION_OUT ?? resolve(root, 'target/wf/bms-video/browser-integration'));
const port = Number(process.env.VIDEO_INTEGRATION_PORT ?? 8131);
const colors = [[255,0,0], [0,255,0], [0,0,255], [255,255,0], [255,0,255],
  [0,255,255], [255,255,255], [96,96,96], [255,0,0], [0,255,0]];
const relativeMs = [0,40,140,180,400,440,600,760,1100,1600];
const evidence = { kind: 'development-actual-video-to-scene', checks: [],
  ceiling: ['Controlled committed preview song times, not physical audio/input cadence.',
    'Public Scene pixels prove rendering; private TextureId/cache allocations are not inspected.',
    'This script is development evidence, not a Harness review or QA receipt.'] };
let browser, server;
const check = (id, detail) => evidence.checks.push({ id, passed: true, detail });

function execute(binary, args) {
  assert(binary, 'Set the explicit BEATKERNEL_TEST_FFMPEG/FFPROBE tool paths.');
  const result = spawnSync(binary, args, { encoding: 'utf8', timeout: 30000, maxBuffer: 4 * 1024 * 1024 });
  assert.equal(result.status, 0, `${binary}: ${result.error ?? result.stderr}`);
  return result.stdout;
}

async function generate() {
  await mkdir(out, { recursive: true });
  const raw = Buffer.alloc(colors.length * 16 * 16 * 4);
  colors.forEach((color, frame) => {
    for (let pixel = 0; pixel < 256; pixel++) raw.set([...color,255], (frame * 256 + pixel) * 4);
  });
  await writeFile(resolve(out, 'colors.rgba'), raw);
  let expression = String(relativeMs.at(-1));
  for (let i = relativeMs.length - 2; i >= 0; i--) expression = `if(eq(N\\,${i})\\,${relativeMs[i]}\\,${expression})`;
  execute(process.env.BEATKERNEL_TEST_FFMPEG, ['-v','error','-y','-f','rawvideo','-pixel_format','rgba',
    '-video_size','16x16','-framerate','25','-i',resolve(out,'colors.rgba'),
    '-vf',`settb=1/1000,setpts=5000+${expression}`,'-fps_mode','passthrough',
    '-c:v','libx264','-pix_fmt','yuv420p','-enc_time_base','1/1000','-crf','12','-bf','2','-g','4',
    '-x264-params','b-adapt=0:scenecut=0','-threads','1','-movflags','+faststart',resolve(out,'vfr.mp4')]);
  const probe = JSON.parse(execute(process.env.BEATKERNEL_TEST_FFPROBE, ['-v','error','-select_streams','v:0',
    '-show_streams','-show_frames','-show_entries','stream=time_base,start_time,has_b_frames:frame=pts,pict_type,key_frame',
    '-of','json',resolve(out,'vfr.mp4')]));
  assert(probe.streams[0].has_b_frames > 0, 'Actual encoded B-frame track required.');
  assert(Number(probe.streams[0].start_time) >= 5, 'Actual nonzero source origin required.');
  const [num,den] = probe.streams[0].time_base.split('/').map(BigInt);
  const origin = BigInt(probe.frames[0].pts);
  const actualMs = probe.frames.map(frame => Number((BigInt(frame.pts)-origin)*num*1000n/den));
  assert.deepEqual(actualMs, relativeMs, 'Encoded fixture must preserve uneven presentation points.');
  assert(probe.frames.some((frame,index) => frame.key_frame && actualMs[index] === 400), '400ms random access point required.');
  assert(probe.frames.some(frame => frame.pict_type === 'B'), 'Actual B-picture required.');
  await writeFile(resolve(out,'fixture-probe.json'), `${JSON.stringify(probe,null,2)}\n`);
  console.log(`Generated ${resolve(out,'vfr.mp4')} with verified VFR, B-frames, origin and seek point.`);
}

// Only this test worker is supplied by the owned server. It imports untouched
// production modules and delegates all preparation, selection and draw calls.
const workerSource = `
import init, { BrowserLibrary, BrowserView } from '/app/web/pkg/beatkernel_bms_runtime.js';
import { VideoClient } from '/app/web/video-client.mjs';
const records = [], errors = [];
let library, prepared, view, canvas, client, codec, sequence = 0n, identity = 1n;
const sleep = ms => new Promise(resolve => setTimeout(resolve,ms));
function assert(value,message) { if (!value) throw Error(message); }
function summary(message) {
  const result = {};
  for (const key of ['type','content','generation','resource','revision','ptsNs','targetNs','completedThroughNs','endNs','reason'])
    if (message?.[key] !== undefined) result[key] = typeof message[key] === 'bigint' ? String(message[key]) : message[key];
  return result;
}
async function initialize(movieURL) {
  await init();
  assert(typeof VideoDecoder === 'function', 'Native Worker WebCodecs unavailable.');
  const bytes = new Uint8Array(await (await fetch(movieURL)).arrayBuffer());
  library = new BrowserLibrary(8,67108864,268435456,4096);
  library.add_file('movie.mp4',bytes);
  // The late playable note establishes lane geometry through a valid WAV
  // definition, rather than relying on undefined sample IDs being accepted.
  const silent = new Uint8Array(44 + 480 * 2 * 2), header = new DataView(silent.buffer);
  const text = (offset,value) => silent.set(new TextEncoder().encode(value),offset);
  text(0,'RIFF'); header.setUint32(4,silent.length-8,true); text(8,'WAVEfmt ');
  header.setUint32(16,16,true); header.setUint16(20,1,true); header.setUint16(22,2,true);
  header.setUint32(24,48000,true); header.setUint32(28,192000,true);
  header.setUint16(32,4,true); header.setUint16(34,16,true);
  text(36,'data'); header.setUint32(40,silent.length-44,true);
  library.add_file('silent.wav',silent);
  library.add_file('video.bms',new TextEncoder().encode('#TITLE Actual video Scene\\n#BPM 120\\n#WAV01 silent.wav\\n#BMP01 movie.mp4\\n#00004:01\\n#00104:01\\n#00411:01\\n'));
  prepared = library.prepare_chart('video.bms',48000,2,1n,67108864,268435456,3844);
  canvas = new OffscreenCanvas(960,720);
  view = await BrowserView.create(canvas);
  codec = new Worker('/app/web/video-decoder-worker.js',{type:'module'});
  codec.addEventListener('message',({data}) => records.push({direction:'received',...summary(data)}));
  const original = codec.postMessage.bind(codec);
  codec.postMessage = (message,transfer) => { records.push({direction:'sent',...summary(message)}); return original(message,transfer); };
  client = new VideoClient({worker:codec,view,onUnavailable:reason=>errors.push(reason)});
  register();
}
function register() {
  sequence = 0n;
  view.import_visual_registration(prepared.visual_registration(identity,identity,16777216,4096),16777216,4096,0);
  client.register(identity,identity,prepared.video_registration());
}
function state(song) {
  sequence++;
  view.import_visual_packet(prepared.preview_state(sequence,BigInt(song)),16777216,4096);
  assert(prepared.acknowledge_visual(identity,identity,sequence),'Actual preview ACK refused.');
  client.refresh();
}
async function image() {
  for (let attempt=0;attempt<8;attempt++) {
    view.draw_visual();
    if (!view.needs_redraw()) break;
    await sleep(20);
  }
  assert(!view.needs_redraw(),'Actual GPU Scene remained unsubmitted.');
  const blob = await canvas.convertToBlob({type:'image/png'});
  const bitmap = await createImageBitmap(blob);
  const copy = new OffscreenCanvas(960,720), context = copy.getContext('2d');
  context.drawImage(bitmap,0,0); bitmap.close();
  const rgba = Array.from(context.getImageData(360,300,1,1).data);
  const digest = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',await blob.arrayBuffer())))
    .map(byte=>byte.toString(16).padStart(2,'0')).join('');
  return {rgba,digest,png:new Uint8Array(await blob.arrayBuffer())};
}
function matches(pixel,wanted) {
  // Existing Scene BGA uses tint/opacity. Require the actual color channels,
  // allowing codec conversion and tint, never equating a black/loading frame.
  const max = Math.max(...pixel.slice(0,3));
  return max > 24 && wanted.every((on,index) => on ? pixel[index] > max*.65 : pixel[index] < max*.35);
}
async function at(song,wanted) {
  state(song);
  const before = records.length;
  let captured;
  for (let attempt=0;attempt<160;attempt++) {
    assert(errors.length===0,errors.join('; '));
    captured = await image();
    const active = client.sessions;
    const allComplete = [...active.values()].every(demand=>demand.completedTarget===demand.targetNs);
    if (active.size && allComplete && matches(captured.rgba,wanted)) return {song:String(song),...captured,records:records.slice(before),demands:view.video_demands().map(summary)};
    await sleep(25);
  }
  throw Error('Expected Scene color '+wanted+' at committed song '+song+'; actual '+captured?.rgba+'; codec '+JSON.stringify(records.slice(-12)));
}
self.onmessage = async ({data}) => {
  try {
    let result;
    if (data.kind==='init') { await initialize(data.movieURL); result={nativeWebCodecs:true}; }
    else if (data.kind==='at') result=await at(data.song,data.wanted);
    else if (data.kind==='pause') {
      const before=await image(), count=records.filter(x=>x.direction==='sent'&&x.type==='demand').length;
      await sleep(300); client.refresh(); const after=await image();
      assert(before.digest===after.digest,'Scene progressed without committed song state.');
      assert(count===records.filter(x=>x.direction==='sent'&&x.type==='demand').length,'Paused target decoded again.');
      result={rgba:after.rgba,digest:after.digest,demandCount:count};
    } else if (data.kind==='replace') { identity++; register(); result=await at(10000000n,[1,0,0]); }
    else if (data.kind==='suspend') {
      client.suspend(); assert(view.video_demands().length===0,'Suspended generation still demanded frames.');
      client.resume(); result=await at(130000000n,[0,1,0]);
    } else if (data.kind==='close') {
      client.close(); view.retire_visual(); view.free(); prepared.free(); library.free();
      await sleep(100); codec.terminate(); result={records,errors,closed:true};
    } else throw Error('Unknown fixture operation.');
    const transfer=result.png?[result.png.buffer]:[];
    self.postMessage({id:data.id,ok:true,result},transfer);
  } catch(error) { self.postMessage({id:data.id,ok:false,error:String(error?.stack??error)}); }
};
`;

async function serve() {
  const cert = process.env.TLS_CERT ?? resolve(out,'cert.pem');
  const key = process.env.TLS_KEY ?? resolve(out,'key.pem');
  if (!process.env.TLS_CERT && !process.env.TLS_KEY) execute('openssl',['req','-x509','-newkey','ec','-pkeyopt',
    'ec_paramgen_curve:prime256v1','-nodes','-keyout',key,'-out',cert,'-days','1','-subj','/CN=localhost',
    '-addext','subjectAltName=IP:127.0.0.1,DNS:localhost']);
  const certificate = new X509Certificate(await readFile(cert));
  const spki = createHash('sha256').update(certificate.publicKey.export({type:'spki',format:'der'})).digest('base64');
  const mime = {'.mjs':'text/javascript','.js':'text/javascript','.wasm':'application/wasm','.html':'text/html','.mp4':'video/mp4'};
  server = createServer({cert:await readFile(cert),key:await readFile(key)},async (request,response) => {
    try {
      const pathname = new URL(request.url,'https://localhost').pathname;
      let bytes, type;
      if (pathname==='/video-fixture-worker.mjs') { bytes=workerSource; type='text/javascript'; }
      else if (pathname==='/video-fixture.html') { bytes='<title>Actual video Scene fixture</title>'; type='text/html'; }
      else if (pathname==='/fixture.mp4') { bytes=await readFile(resolve(out,'vfr.mp4')); type='video/mp4'; }
      else {
        const path=resolve(root,`.${decodeURIComponent(pathname)}`), local=relative(root,path);
        assert(local!=='..'&&!local.startsWith(`..${sep}`),'Outside repository');
        bytes=await readFile(path); type=mime[extname(path)]??'application/octet-stream';
      }
      response.writeHead(200,{'Content-Type':type,'Cross-Origin-Opener-Policy':'same-origin',
        'Cross-Origin-Embedder-Policy':'require-corp','Cache-Control':'no-store'}); response.end(bytes);
    } catch { response.writeHead(404); response.end(); }
  });
  await new Promise((yes,no)=>{server.once('error',no);server.listen(port,'127.0.0.1',yes);});
  return spki;
}

async function run() {
  await mkdir(out,{recursive:true});
  // Running never substitutes a fake movie if actual fixture generation was skipped.
  evidence.fixtureProbe=JSON.parse(await readFile(resolve(out,'fixture-probe.json'),'utf8'));
  const spki=await serve(), require=createRequire(import.meta.url);
  browser=await require(process.env.PUPPETEER_MODULE??'puppeteer-core').launch({
    executablePath:process.env.CHROMIUM??'/usr/bin/chromium',headless:true,
    args:['--no-sandbox','--disable-dev-shm-usage','--enable-unsafe-webgpu',
      '--use-angle=swiftshader','--enable-features=Vulkan','--use-vulkan=swiftshader',
      '--autoplay-policy=no-user-gesture-required',`--ignore-certificate-errors-spki-list=${spki}`]});
  const page=await browser.newPage();
  await page.goto(`https://127.0.0.1:${port}/video-fixture.html`);
  await page.evaluate(()=>{
    const worker=globalThis.fixtureWorker=new Worker('/video-fixture-worker.mjs',{type:'module'});
    let next=0; const pending=new Map();
    worker.onmessage=({data})=>{const entry=pending.get(data.id);if(!entry)return;clearTimeout(entry.timer);pending.delete(data.id);data.ok?entry.yes(data.result):entry.no(Error(data.error));};
    worker.onerror=event=>{for(const entry of pending.values()){clearTimeout(entry.timer);entry.no(Error(event.message));}pending.clear();};
    globalThis.fixtureCall=message=>new Promise((yes,no)=>{const id=++next,timer=setTimeout(()=>{pending.delete(id);no(Error('Actual video fixture timed out.'));},30000);pending.set(id,{yes,no,timer});worker.postMessage({...message,id});});
  });
  const call=message=>page.evaluate(message=>fixtureCall(message),message);
  check('actual-webcodecs-worker',await call({kind:'init',movieURL:'/fixture.mp4'}));
  for (const [id,song,wanted] of [
    ['gap-preroll-green',130000000,[0,1,0]], ['vfr-gap-yellow',300000000,[1,1,0]],
    ['keyframe-seek-white',650000000,[1,1,1]], ['backward-seek-green',130000000,[0,1,0]],
    ['eof-held-green',1900000000,[0,1,0]], ['equal-image-marker-restart-red',2010000000,[1,0,0]],
    ['equal-image-marker-green',2050000000,[0,1,0]],
  ]) {
    const result=await call({kind:'at',song,wanted});
    await writeFile(resolve(out,`${id}.png`),Buffer.from(Object.values(result.png)));
    delete result.png; check(id,result);
    if(id==='backward-seek-green') check('pause-fixed-song-state',await call({kind:'pause'}));
  }
  for (const [id,kind] of [['new-content-generation','replace'],['suspend-resume-generation','suspend']]) {
    const result=await call({kind});delete result.png;check(id,result);
  }
  const closed=await call({kind:'close'});
  assert.deepEqual(closed.errors,[]);
  const sentFrames=closed.records.filter(record=>record.direction==='received'&&record.type==='frame');
  const acks=closed.records.filter(record=>record.direction==='sent'&&record.type==='ack');
  assert(sentFrames.length>0,'No actual native WebCodecs frame traversed production client.');
  assert.equal(acks.length,sentFrames.length,'Every transferred frame must return one independent credit.');
  check('actual-frame-credit-cleanup',{frames:sentFrames.length,acks:acks.length,closed:closed.closed});
  evidence.transport=closed.records;
  await page.evaluate(()=>fixtureWorker.terminate());
}

if(process.argv.includes('--generate')) await generate();
else {
  try {await run();evidence.passed=true;}
  catch(error){evidence.passed=false;evidence.error=String(error?.stack??error);process.exitCode=1;}
  finally {
    await browser?.close();
    if(server) await new Promise(resolveClose=>server.close(resolveClose));
    await mkdir(out,{recursive:true});
    await writeFile(resolve(out,'evidence.json'),`${JSON.stringify(evidence,null,2)}\n`);
    console.log(JSON.stringify({passed:evidence.passed,checks:evidence.checks.length,error:evidence.error,evidence:resolve(out,'evidence.json')}));
  }
}
