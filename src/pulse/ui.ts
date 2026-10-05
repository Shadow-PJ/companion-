import { el } from "../shared/dom";
const paths: Record<string,string> = {
  pulse: "M2 12h4l3-8 5 16 3-8h5", home: "m3 10 9-7 9 7v10H3Z M9 20v-7h6v7",
  news: "M4 3h13v18H4Z M17 7h4v14h-4 M7 7h7 M7 11h7 M7 15h7 M7 18h4",
  models: "m12 3 9 5-9 5-9-5Z m-9 10 9 5 9-5 m-18 5 9 5 9-5",
  arena: "m4 3 16 17 m0-17L4 20 M3 14l7 7 M14 3l7 7", tools: "m14 3 7 7-11 11-7-7Z M14 3l-3 6 4 4 6-3",
  clock: "M12 8v5l3 2 M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0",
  saved: "M6 3h12v18l-6-4-6 4Z", settings: "M12 3v3 M12 18v3 M3 12h3 M18 12h3 m-15-6 2 2 m8 8 2 2 m0-12-2 2 m-8 8-2 2 M17 12a5 5 0 1 1-10 0 5 5 0 0 1 10 0",
  search: "M16 16l5 5 M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0", bell: "M5 17h14l-2-3V9a5 5 0 0 0-10 0v5Z M9 20h6",
  arrow: "M5 12h14 m-6-6 6 6-6 6", external: "M14 3h7v7 M21 3l-9 9 M10 3H3v18h18v-7", close: "m6 6 12 12 M6 18 18 6",
  refresh: "M20 8A9 9 0 1 0 21 15 M20 3v6h-6", check: "m5 12 4 4 10-10", plus: "M12 5v14 M5 12h14",
  moon: "M20 15A9 9 0 1 1 9 3a7 7 0 0 0 11 12", sun: "M12 2v2 M12 20v2 M2 12h2 M20 12h2 m-16-6 1 1 m10 10 1 1 m0-12-1 1 m-10 10-1 1 M16 12a4 4 0 1 1-8 0 4 4 0 0 1 8 0",
  menu: "M4 6h16 M4 12h16 M4 18h16", code: "m8 6-6 6 6 6 m8-12 6 6-6 6 m-3-15-2 18",
  sparkle: "m12 2 3 7 7 3-7 3-3 7-3-7-7-3 7-3Z", image: "M3 3h18v18H3Z m0 14 6-6 5 5 4-4 3 3 M8 7h.01",
  video: "M3 5h13v14H3Z m13 4 6-4v14l-6-4", audio: "M9 18V5l11-2v14 M3 18a3 3 0 1 0 6 0 3 3 0 0 0-6 0 M14 17a3 3 0 1 0 6 0 3 3 0 0 0-6 0",
  study: "m2 8 10-5 10 5-10 5Z M6 10v7l6 3 6-3v-7 M22 8v9", research: "M10 4v7L4 20h16l-6-9V4 M8 4h8 M7 16h10",
  data: "M5 20V10 M12 20V4 M19 20v-7", writing: "m4 16 12-12 4 4L8 20H4Z M14 6l4 4",
  agents: "M5 7h14v13H5Z M12 7V3 M9 12h.01 M15 12h.01 M9 16h6 M2 11v5 M22 11v5",
  presentations: "M3 4h18v13H3Z M12 17v5 M7 22l5-5 5 5 M7 12l3-3 3 2 4-4",
};
export function icon(name: string, size=18): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  for (const [k,v] of Object.entries({width:String(size),height:String(size),viewBox:"0 0 24 24",fill:"none",stroke:"currentColor","stroke-width":"1.6","stroke-linecap":"round","stroke-linejoin":"round","aria-hidden":"true"})) svg.setAttribute(k,v);
  const path = document.createElementNS(svg.namespaceURI, "path"); path.setAttribute("d",paths[name] ?? paths.sparkle); svg.append(path); return svg;
}
export function btn(label: string, action: ()=>void, cls="", glyph?:string) {
  const b=el("button",{type:"button",class:`button ${cls}`,"aria-label":label},glyph?icon(glyph):null,label); b.addEventListener("click",action);return b;
}
export function iconButton(label:string,glyph:string,action:()=>void) {
  const b=el("button",{type:"button",class:"icon-button",title:label,"aria-label":label},icon(glyph));b.addEventListener("click",action);return b;
}
export function pill(text:string, cls="") { return el("span",{class:`pill ${cls}`,text}); }
export function stamp(value:string) {
  const date = new Date(value); if(!Number.isFinite(date.getTime()))return "Not checked";
  return date.toLocaleDateString(undefined,{month:"short",day:"numeric",year:"numeric"});
}
export function relative(value:string) {
  const time=Date.parse(value); if(!Number.isFinite(time))return "Undated";
  const hours=Math.max(0,(Date.now()-time)/3600000);
  return hours<1 ? "Just now" : hours<24 ? `${Math.floor(hours)}h ago` : hours<24*7 ? `${Math.floor(hours/24)}d ago` : stamp(value);
}
export function money(value:number|null) { return value===null ? "Not verified" : `$${value.toLocaleString(undefined,{maximumFractionDigits:2})}`; }
export function mark(name:string, cls="") {
  const logos:Record<string,string>={OpenAI:"O",Anthropic:"A",Google:"G","Hugging Face":"H",Meta:"M",Cursor:"C",Cognition:"W",Microsoft:"M"};
  return el("span",{class:`brand-mark ${cls}`,text:logos[name] ?? name.slice(0,1).toUpperCase(),"aria-hidden":"true"});
}
export function empty(title:string,detail:string,action?:HTMLElement) {
  return el("div",{class:"empty"},icon("search",26),el("h3",{text:title}),el("p",{text:detail}),action);
}
