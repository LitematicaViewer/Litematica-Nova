import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export function invoke<T = unknown>(command: string, args?: Record<string, unknown>): Promise<T> {
  // #region agent log
  const startTime = Date.now();
  const logData = {sessionId:'eabd7817-9767-4d77-8cb1-446d598a0056',location:'tauri.ts:4',message:'invoke called',data:{command,argsKeys:args?Object.keys(args):[]},timestamp:Date.now(),hypothesisId:'B,C'};
  console.log('[DEBUG]', logData);
  fetch('http://127.0.0.1:7337/ingest/eabd7817-9767-4d77-8cb1-446d598a0056',{method:'POST',headers:{'Content-Type':'application/json','X-Debug-Session-Id':'eabd7817-9767-4d77-8cb1-446d598a0056'},body:JSON.stringify(logData)}).catch(()=>{});
  // #endregion
  
  return tauriInvoke<T>(command, args).then(result => {
    // #region agent log
    const endTime = Date.now();
    const logData = {sessionId:'eabd7817-9767-4d77-8cb1-446d598a0056',location:'tauri.ts:11',message:'invoke completed',data:{command,duration:endTime-startTime},timestamp:Date.now(),hypothesisId:'B,C'};
    console.log('[DEBUG]', logData);
    fetch('http://127.0.0.1:7337/ingest/eabd7817-9767-4d77-8cb1-446d598a0056',{method:'POST',headers:{'Content-Type':'application/json','X-Debug-Session-Id':'eabd7817-9767-4d77-8cb1-446d598a0056'},body:JSON.stringify(logData)}).catch(()=>{});
    // #endregion
    return result;
  }).catch(err => {
    // #region agent log
    const endTime = Date.now();
    const logData = {sessionId:'eabd7817-9767-4d77-8cb1-446d598a0056',location:'tauri.ts:19',message:'invoke failed',data:{command,duration:endTime-startTime,error:String(err)},timestamp:Date.now(),hypothesisId:'B,C'};
    console.log('[DEBUG]', logData);
    fetch('http://127.0.0.1:7337/ingest/eabd7817-9767-4d77-8cb1-446d598a0056',{method:'POST',headers:{'Content-Type':'application/json','X-Debug-Session-Id':'eabd7817-9767-4d77-8cb1-446d598a0056'},body:JSON.stringify(logData)}).catch(()=>{});
    // #endregion
    throw err;
  });
}
