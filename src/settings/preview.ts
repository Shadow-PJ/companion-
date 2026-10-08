// Browser-only fixtures are isolated from the desktop app and agent settings.
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { Settings } from "../shared/types";
import { defaultPreferences } from "../pulse/types";
import catalog from "../pulse/catalog.json";
export const desktopSettings = isTauri();
const defaults: Settings = {
  pet:{monitor:"",character:"",position:.5,statusLine:true,followMouse:true,interactions:true,reducedMotion:false,showOnPermission:true,showOnDone:true,showOnAttention:true},
  permissions:{enabled:true,timeoutSecs:60},chat:{enabled:true,showReplies:true,projectDir:"",mode:"ask",keepConversation:true,agent:"auto",claudePath:""},gameMode:true,
  quickActions:{enabled:true,actions:[{id:"explain-error",label:"Explain the last error",prompt:"Explain {last_error} in {project}.",readOnly:true}]},dropFiles:true,errorWatcher:false,health:true,
  progression:{enabled:true,neglect:true,hat:"",color:"mint",aura:"",pet:"ninja"},breaks:{enabled:true,intervalMins:60},sounds:{enabled:true,volume:40,taskDone:true,needsYou:true,problems:true,levelUp:true,breaks:true},
  briefing:{enabled:true},learn:{enabled:true,everyMins:30},quests:{enabled:true,difficulty:"normal",perDay:3,kinds:["fix","test","tasks"]},github:{enabled:false,everyMins:15},squad:{enabled:false,maxShown:3},
  autoAllow:{full:false,never:["git push","rm","Remove-Item"],outsideProject:true},limits:{enabled:true,warn:true,warnPercent:80},detective:{enabled:true,weekly:true,cacheReminder:true,guard:true,guardMinTokens:150000,bigChat:true,bigChatTokens:400000},alerts:{windowsNotifications:true}
};
let previewSettings = structuredClone(defaults);
try { const saved=localStorage.getItem("glowby-settings-preview"); if(saved)previewSettings={...defaults,...JSON.parse(saved)}; }catch{/* defaults */}
const emptyLimits={agents:[{agent:"Claude",plan:"",windows:[],emptyHint:"Usage comes from your local Claude Code session in the desktop app."},{agent:"Codex",plan:"",windows:[],emptyHint:"Usage comes from your local Codex session in the desktop app."}]};
const progress={view:{level:7,stage:1,stageName:"Lantern Glowby",xp:1150,xpIntoLevel:100,xpForLevel:350,streak:5,energy:100},look:{stage:1,hat:"",color:"mint",aura:"",weak:false,character:"",species:"ninja"},stats:{tasks:20,fixes:3,testsPassed:5,commits:7,breaks:4,pets:12},bestStreak:5,stages:[[1,"Little"],[6,"Lantern"],[15,"Starlit"],[30,"Aurora"]],cosmetics:[...["mint","periwinkle","peach"].map(id=>({id,kind:"color",name:id,unlocked:true,requirement:""})),...["ninja","neko","kitsune"].map(id=>({id,kind:"pet",name:id,unlocked:true,requirement:""})),...["wave","heart","dance"].map(id=>({id,kind:"emote",name:id,unlocked:true,requirement:""}))]};
export async function settingsInvoke<T>(command:string,args?:Parameters<typeof invoke>[1],options?:Parameters<typeof invoke>[2]):Promise<T> {
  if(desktopSettings)return invoke<T>(command,args,options);
  const input=(args??{}) as Record<string,unknown>;
  if(command==="get_settings")return structuredClone(previewSettings) as T;
  if(command==="save_settings"){previewSettings=structuredClone(input.settings as Settings);localStorage.setItem("glowby-settings-preview",JSON.stringify(previewSettings));return previewSettings as T;}
  if(command==="app_info")return {version:"0.6.1 · preview",dataDir:"Local desktop app data",claudePath:null,fullscreenNow:false,pipeError:null} as T;
  if(command==="list_monitors")return [{name:"",label:"Primary monitor",primary:true}] as T;
  if(command==="get_progress")return progress as T;
  if(command==="limits_refresh")return emptyLimits as T;
  if(command==="hooks_status"||command==="codex_hooks_status")return {state:"installed",detail:"Browser preview",settingsPath:command==="hooks_status"?"~/.claude/settings.json":"~/.codex/hooks.json",hookPath:"Glowby-owned hooks",backupsDir:"Local backups"} as T;
  if(command.endsWith("hooks_preview"))return {install:input.install,changed:false,diff:[],token:"",settingsPath:"Desktop settings",reformatted:false} as T;
  if(command==="characters_list"||command==="auto_allow_log"||command==="quests_today")return [] as T;
  if(command==="auto_allow_defaults")return defaults.autoAllow.never as T;
  if(command==="default_quick_actions")return structuredClone(defaults.quickActions.actions) as T;
  if(command==="detective_last")return "No case report in browser preview. Reports use your desktop session logs." as T;
  if(command==="github_status")return {hasToken:false,repos:[],error:null,lastChecked:null} as T;
  if(command==="pulse_view"||command==="pulse_preferences"){
    let preferences={...defaultPreferences};try{preferences={...preferences,...JSON.parse(localStorage.getItem("glowby-pulse-preview-preferences")??"{}")};}catch{/* defaults */}
    if(command==="pulse_preferences"){preferences={...preferences,...input.patch as object};localStorage.setItem("glowby-pulse-preview-preferences",JSON.stringify(preferences));return preferences as T;}
    return {cache:{...catalog,benchmarks:[],sources:[],changes:[],lastChecked:""},preferences,refreshing:false} as T;
  }
  throw new Error("This action is available in the desktop app. Browser preview does not connect to your agents or modify their settings.");
}
