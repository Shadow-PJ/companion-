import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { el } from "../shared/dom";
import catalog from "./catalog.json";
import { benchmarkFor, briefing as buildBriefing, ranked, search } from "./search";
import { categories, defaultPreferences, topics } from "./types";
import type { Article, Cache, Model, Page, Preferences, Tool, View } from "./types";
import { btn, empty, icon, iconButton, mark, money, pill, relative, stamp } from "./ui";


const desktop = isTauri();
const nav: {id:Page;name:string;glyph:string;key:string}[] = [
  {id:"home",name:"Home",glyph:"home",key:"1"}, {id:"news",name:"News",glyph:"news",key:"2"},
  {id:"models",name:"Models",glyph:"models",key:"3"}, {id:"arena",name:"Compare",glyph:"arena",key:"4"},
  {id:"tools",name:"Explore AI",glyph:"tools",key:"5"}, {id:"releases",name:"Releases",glyph:"clock",key:"6"},
  {id:"saved",name:"Saved",glyph:"saved",key:"7"},
];
let view: View = {cache:{...catalog,benchmarks:[],sources:[],lastChecked:"",changes:[],lastAttempt:0} as Cache,preferences:{...defaultPreferences},refreshing:false};
let page: Page = "home", topic="All", company="All", metric="Intelligence", toolCategory="All", freeOnly=false;
let query="", left="claude-sonnet-5-5", right="gpt-6.1-sol", pending=false;
let inputTokens=100000,outputTokens=10000;
const content=el("main",{id:"content",tabindex:"-1"});
const syncStatus=el("span",{class:"sync-status"});
const sidebar=el("aside",{class:"sidebar","aria-label":"Main navigation"});
const toast=el("div",{class:"snackbar",role:"status","aria-live":"polite",hidden:true});
const unread=el("span",{class:"unread-dot",hidden:true});
const breadcrumb=el("span",{class:"breadcrumb",text:"AI Pulse"});
let toastTimer=0;

function message(text:string) { toast.textContent=text;toast.hidden=false;window.clearTimeout(toastTimer);toastTimer=window.setTimeout(()=>toast.hidden=true,5000); }
function source(url:string) { if(desktop)void invoke("pulse_open_source",{url}).catch(e=>message(String(e)));else window.open(url,"_blank","noopener,noreferrer"); }
function sourceButton(url:string,label="Original source") { return btn(label,()=>source(url),"quiet","external"); }
function articles() { return [...view.cache.articles,...view.cache.changes].sort((a,b)=>Date.parse(b.published)-Date.parse(a.published)); }
function follows(a:Article) {
  const p=view.preferences;
  return (!p.companies.length||p.companies.includes(a.company))&&(!p.topics.length||a.tags.some(t=>p.topics.includes(t)))&&(!p.families.length||p.families.some(f=>a.title.toLowerCase().includes(f.toLowerCase())));
}
function watched() { return articles().filter(follows); }
function unreadArticles() { return watched().filter(a=>!view.preferences.read.includes(a.id)); }
function jump(next:Page) { page=next;sidebar.classList.remove("mobile-open");render();content.scrollTop=0; }

async function preferences(change:Partial<Preferences>) {
  const next={...view.preferences,...change};
  try {
    const saved=desktop?await invoke<Preferences>("pulse_preferences",{patch:change}):next;
    if(!desktop)localStorage.setItem("glowby-pulse-preview-preferences",JSON.stringify(saved));
    view.preferences=saved;applyTheme();render();return true;
  }catch(e){message(String(e));return false;}
}
function toggleSaved(id:string) {
  const current=view.preferences.saved;
  return preferences({saved:current.includes(id)?current.filter(x=>x!==id):[...current,id]});
}
function toggleCompany(name:string) {
  const companies=view.preferences.companies;
  void preferences({companies:companies.includes(name)?companies.filter(c=>c!==name):[...companies,name]});
}
function saveButton(id:string) {
  const saved=view.preferences.saved.includes(id);
  const button=iconButton(saved?"Remove from saved":"Save for later",saved?"check":"saved",()=>void toggleSaved(id).then(ok=>{
    if(!ok)return;const selected=view.preferences.saved.includes(id);button.replaceChildren(icon(selected?"check":"saved"));button.setAttribute("aria-label",selected?"Remove from saved":"Save for later");button.title=selected?"Remove from saved":"Save for later";
  }));return button;
}
function applyTheme() { document.documentElement.dataset.theme=view.preferences.theme; }
function sectionHeading(title:string,sub?:string,action?:HTMLElement) {
  return el("div",{class:"section-heading"},el("div",{},el("h2",{text:title}),sub?el("p",{text:sub}):null),action);
}
function filters(values:string[],chosen:string,change:(value:string)=>void) {
  return el("div",{class:"tabs",role:"group"},...values.map(value=>{
    const b=btn(value,()=>change(value),value===chosen?"selected":"");b.setAttribute("aria-pressed",String(value===chosen));return b;
  }));
}
function select(label:string,values:{value:string;label:string}[],chosen:string,change:(value:string)=>void) {
  const s=el("select",{"aria-label":label},...values.map(v=>el("option",{value:v.value,text:v.label})));s.value=chosen;s.addEventListener("change",()=>change(s.value));return s;
}
function skeleton() { return el("div",{class:"skeleton-grid","aria-label":"Loading AI Pulse"},...Array.from({length:5},()=>el("div",{class:"skeleton"}))); }

function shell() {
  const brand=el("div",{class:"brand"},el("div",{class:"glowby-logo"},icon("pulse",24)),el("strong",{text:"glowby"}),el("span",{text:"PULSE",class:"brand-tag"}));
  const navigation=el("nav",{},...nav.map(n=>{
    const b=btn(n.name,()=>jump(n.id),"nav-item",n.glyph);b.dataset.page=n.id;b.title=`Alt + ${n.key}`;return b;
  }));
  const following=el("div",{class:"following"},el("div",{class:"sidebar-label",text:"YOUR SIGNAL"}),...['OpenAI','Anthropic','Google'].map(name=>{
    const b=btn(name,()=>{company=name;topic="All";jump("news");},"company-link");b.prepend(mark(name,"small"));return b;
  }),btn("Manage watchlist",()=>jump("settings"),"manage-link","plus"));
  sidebar.append(brand,navigation,following,el("div",{class:"sidebar-footer"},btn("Preferences",()=>jump("settings"),"nav-item","settings"),el("div",{class:"local-note"},icon("check",12),"Personal. Local. Source-backed.")));
  const command=btn("Search or ask anything…",()=>palette(),"top-search","search");command.append(el("kbd",{text:"Ctrl K"}));
  const alerts=iconButton("Your watchlist updates","bell",()=>notifications());alerts.append(unread);
  const topbar=el("header",{class:"topbar"},iconButton("Toggle sidebar","menu",()=>sidebar.classList.toggle("mobile-open")),breadcrumb,command,el("div",{class:"top-actions"},syncStatus,iconButton("Change theme","moon",()=>void preferences({theme:document.documentElement.dataset.theme==="dark"?"light":"dark"})),alerts,btn("PJ",()=>jump("settings"),"avatar")));
  document.getElementById("app")!.replaceChildren(sidebar,el("div",{class:"workspace"},topbar,content),toast);
}

function articleRow(a:Article,featured=false) {
  const open=btn(a.title,()=>detail(a),"article-title");
  const row=el("article",{class:`article-row ${featured?"featured":""}`},mark(a.company),el("div",{class:"article-body"},
    el("div",{class:"meta"},el("span",{text:a.company}),el("span",{text:"·"}),el("time",{datetime:a.published,text:relative(a.published)}),a.impact==="High"?pill("High impact","accent"):null),
    open,a.summary?el("p",{class:"excerpt",text:a.summary}):null,
    el("div",{class:"article-bottom"},el("div",{class:"tag-list"},...a.tags.slice(0,3).map(t=>pill(t))),btn("Why it matters",()=>detail(a),"text-link","arrow"))),saveButton(a.id));
  return row;
}
function detail(a:Article) {
  if(!view.preferences.read.includes(a.id))void preferences({read:[...view.preferences.read,a.id]});
  const following=view.preferences.companies.includes(a.company);
  openDialog(el("div",{class:"detail-content"},el("div",{class:"meta"},mark(a.company),a.company,"·",stamp(a.published),pill(a.impact,a.impact==="High"?"accent":"")),
    el("h1",{text:a.title}),el("div",{class:"tag-list"},...a.tags.map(t=>pill(t))),
    el("h3",{text:a.summary?"From the publisher":"What happened"}),el("p",{text:a.summary||a.title}),
    el("h3",{text:"Why it matters"}),el("p",{text:a.why}),el("p",{class:"fine-print",text:"Glowby's take: an automatic reading suggestion based on the topic, not a verified impact assessment."}),
    el("div",{class:"detail-actions"},sourceButton(a.url),btn(view.preferences.saved.includes(a.id)?"Saved":"Save",()=>toggleSaved(a.id),"","saved"),btn(following?`Following ${a.company}`:`Follow ${a.company}`,()=>{toggleCompany(a.company);closeDialog();},"","plus"),a.tags.includes("Models")?btn("Compare models",()=>{closeDialog();jump("arena");},"","arena"):null),
    el("div",{class:"source-foot"},el("strong",{text:a.source}),el("p",{text:`Published ${stamp(a.published)} · fetched ${stamp(a.checked)}. Headline and short excerpt from the original publisher. Follow the source for the full details.`}))),"News detail");
}

function home() {
  const hour=new Date().getHours(),greeting=hour<12?"Good morning":hour<18?"Good afternoon":"Good evening";
  const today=new Date().toLocaleDateString(undefined,{weekday:"long",month:"long",day:"numeric"});
  const recent=watched();const brief=buildBriefing(recent); const top=ranked(view.cache,"Intelligence")[0];
  const input=el("input",{type:"search",placeholder:"Ask anything about AI…","aria-label":"Ask anything about AI",autocomplete:"off"});
  const form=el("form",{class:"ask-bar"},icon("sparkle",21),input,btn("Ask Glowby",()=>ask(input.value),"primary","arrow"));
  form.addEventListener("submit",e=>{e.preventDefault();ask(input.value);});
  const intro=el("section",{class:"home-intro"},el("div",{class:"eyebrow"},el("span",{class:"live-dot"}),"YOUR DAILY AI SIGNAL",el("span",{class:"intro-date",text:today})),
    el("h1",{},greeting,el("span",{class:"muted",text:". Stay a step ahead."})),el("p",{text:"The updates worth your attention. The context to make them useful."}),form,
    el("div",{class:"suggestions"},el("span",{text:"Try asking"}),...['What happened with OpenAI this week?','GPT vs Claude for React','Free AI for making videos'].map(q=>btn(q,()=>ask(q),"suggestion"))));
  const summary=el("div",{class:"summary-strip"},
    el("div",{class:"summary-cell"},icon("pulse"),el("span",{class:"tiny-label",text:"ON YOUR RADAR"}),el("strong",{text:String(unreadArticles().length)}),el("span",{text:"unread updates"})),
    el("div",{class:"summary-cell"},icon("models"),el("span",{class:"tiny-label",text:"INTELLIGENCE INDEX"}),el("strong",{class:"summary-model",text:top?.name.split(" (")[0]??"Check the latest"}),el("span",{text:top?"Leading the measured snapshot":"Refresh to load the benchmark"})),
    el("div",{class:"summary-cell"},icon("tools"),el("span",{class:"tiny-label",text:"WORTH EXPLORING"}),el("strong",{text:`${view.cache.tools.length} tools`}),el("span",{text:"Across ten kinds of work"})),
  );
  const briefing=el("section",{class:"briefing-panel"},sectionHeading("Your 60-second briefing","Five things from your watchlist",btn("Read the feed",()=>jump("news"),"text-link","arrow")),
    ...brief.map((a,i)=>el("button",{type:"button",class:"briefing-item","aria-label":a.title},el("span",{class:"brief-number",text:String(i+1).padStart(2,"0")}),el("span",{class:"brief-copy"},el("strong",{text:a.title}),el("span",{text:`${a.company} · ${relative(a.published)}`})),icon("arrow",15))));
  briefing.querySelectorAll<HTMLButtonElement>(".briefing-item").forEach((b,i)=>b.addEventListener("click",()=>detail(brief[i])));
  if(!brief.length)briefing.append(empty("Your watchlist is quiet","Refresh the sources, or follow more companies in Preferences."));
  const trending=el("section",{class:"trending"},sectionHeading("In the conversation","Recent announcements from your sources",btn("All news",()=>jump("news"),"text-link","arrow")),...recent.filter(a=>a.kind!=="release").slice(0,4).map(a=>articleRow(a)));
  const fitting=view.cache.tools.filter(t=>t.categories.some(c=>view.preferences.topics.includes(c)));
  const choices=fitting.length?fitting:view.cache.tools;
  const todayPick=choices[Math.floor(Date.now()/86400000)%choices.length];
  const rail=el("aside",{class:"right-rail"},
    el("section",{class:"rail-section"},sectionHeading("Model movement"),...(view.cache.benchmarks.length?ranked(view.cache,"Intelligence").slice(0,4).map(b=>el("div",{class:"movement-row"},mark(b.company,"small"),el("span",{text:b.name.split(" (")[0]}),el("strong",{class:b.movement&&b.movement>0?"positive":"muted",text:b.movement===null?"—":b.movement===0?"→":`${b.movement>0?"↑":"↓"} ${Math.abs(b.movement)}`}))):[el("p",{class:"muted",text:"Refresh to start tracking measured positions."})]),el("p",{class:"fine-print",text:"Movement between your last two successful snapshots. First check establishes a baseline."})),
    el("section",{class:"try-panel"},icon("sparkle",22),el("div",{class:"eyebrow",text:"WORTH TRYING"}),el("h3",{text:todayPick?.name??"Explore your next tool"}),el("p",{text:todayPick?.bestFor??"Find a tool by the task you need to do."}),todayPick?el("p",{class:"fine-print",text:todayPick.price}):null,btn("See why & alternatives",()=>todayPick?toolDetail(todayPick):jump("tools"),"","arrow")),
    el("section",{class:"rail-section"},sectionHeading("Fresh releases"),...articles().filter(a=>a.kind==="model"||a.kind==="release").slice(0,3).map(a=>{
      const b=btn(a.title,()=>detail(a),"release-mini");b.append(el("span",{class:"muted",text:`${a.company} · ${relative(a.published)}`}));return b;
    }),btn("Release timeline",()=>jump("releases"),"text-link","arrow")),
  );
  const lead=el("section",{class:"home-leaderboard"},sectionHeading("Model leaderboard","One view of quality, speed and task cost",btn("Compare models",()=>jump("arena"),"text-link","arena")),leaderboard(true));
  content.append(intro,summary,el("div",{class:"home-columns"},el("div",{class:"main-column"},briefing,trending),rail),lead);
}

function news(saved=false,releases=false) {
  title(saved?"Your saved signal":releases?"Release timeline":"The AI news desk",saved?"The announcements and tools you wanted to come back to.":releases?"Model launches and software releases, linked to the original announcement.":"Announcements from the people building it. Context from Glowby.");
  if(saved) {
    const list=articles().filter(a=>view.preferences.saved.includes(a.id)),tools=view.cache.tools.filter(t=>view.preferences.saved.includes(`tool:${t.id}`));
    if(!list.length&&!tools.length)content.append(empty("Nothing saved yet","Use the bookmark on any news item or tool. Saved material stays on this PC.",btn("Explore the news",()=>jump("news"),"primary")));
    content.append(...list.map(a=>articleRow(a)),...tools.map(t=>toolCard(t)));return;
  }
  content.append(el("div",{class:"filter-bar"},filters(topics,topic,v=>{topic=v;render();}),select("Filter company",["All",...new Set(articles().map(a=>a.company))].map(v=>({value:v,label:v==="All"?"All companies":v})),company,v=>{company=v;render();})));
  const list=articles().filter(a=>(topic==="All"||a.tags.includes(topic))&&(company==="All"||a.company===company)&&(!releases||a.kind==="model"||a.kind==="release"));
  if(!list.length)content.append(empty("No updates match these filters","Try all companies or another topic."));
  let day="";
  for(const a of list.slice(0,100)) {
    const next=stamp(a.published);if(releases&&next!==day){day=next;content.append(el("div",{class:"timeline-day",text:day}));}
    content.append(articleRow(a));
  }
}
function title(heading:string,sub:string) { content.append(el("div",{class:"page-heading"},el("h1",{text:heading}),el("p",{text:sub}))); }

function leaderboard(compact=false) {
  const benchmark=view.cache.benchmarks; const rows=ranked(view.cache,metric,company).slice(0,compact?5:50);
  const box=el("div",{class:"leaderboard"},filters(["Intelligence","Speed","Cost per task"],metric,v=>{metric=v;render();}));
  if(!benchmark.length){box.append(empty("A measured leaderboard, once checked","Refresh to retrieve Artificial Analysis's public table. Glowby won't fill missing scores with guesses.",btn("Check sources",()=>void refresh(),"primary","refresh")));return box;}
  const field=metric==="Speed"?"speed":metric==="Cost per task"?"cost":"intelligence";
  const table=el("table",{},el("thead",{},el("tr",{},...["#","Model & configuration",metric,metric==="Intelligence"?"Cost / task":"Intelligence",metric==="Speed"?"First chunk":"Tokens / sec"].map(t=>el("th",{scope:"col",text:t})))));
  const body=el("tbody");const max=Math.max(...rows.map(b=>b[field]??0),1);
  rows.forEach((b,i)=>{
    const val=b[field];const bar=el("span",{class:"score-bar"});bar.style.width=`${Math.max(4,(val??0)/max*100)}%`;
    const row=el("tr",{},el("td",{class:"rank",text:String(i+1).padStart(2,"0")}),el("td",{},el("div",{class:"model-name"},mark(b.company,"small"),el("span",{},el("strong",{text:b.name.split(" (")[0]}),el("small",{text:b.name.includes(" (")?b.name.slice(b.name.indexOf(" (")+1):b.company})))),el("td",{},el("div",{class:"score"},bar,el("strong",{text:field==="cost"?money(val):val?.toLocaleString()??"—"}))),el("td",{class:"numeric",text:metric==="Intelligence"?money(b.cost):b.intelligence?.toString()??"—"}),el("td",{class:"numeric",text:metric==="Speed"?(b.latency===null?"—":`${b.latency}s`):b.speed?.toString()??"—"}));
    body.append(row);
  });
  table.append(body);box.append(el("div",{class:"table-scroll"},table),el("div",{class:"benchmark-note"},el("span",{text:`Artificial Analysis Intelligence Index · checked ${relative(benchmark[0].checked)}. Cost = benchmark task USD, not API token pricing. Configurations differ. This is not a coding-only score.`}),sourceButton("https://artificialanalysis.ai/leaderboards/models","Method & full table")));
  return box;
}
function modelsPage() {
  title("Find a model for the task","Published specifications and independently measured snapshots. Every number has a source.");
  company="All";content.append(leaderboard(),sectionHeading("Model reference","API prices are a dated reference, not a live billing quote."));
  const grid=el("div",{class:"model-grid"},...view.cache.models.map(m=>el("article",{class:"model-card"},el("div",{class:"meta"},mark(m.company),m.company),el("h3",{text:m.name}),el("p",{text:m.bestFor}),el("div",{class:"model-specs"},el("span",{},"Input ",el("strong",{text:money(m.inputPrice)})),el("span",{},"Output ",el("strong",{text:money(m.outputPrice)}))),el("span",{class:"fine-print",text:`USD / 1M tokens · checked ${stamp(m.checked)}`}),btn("Compare",()=>{left=m.id;jump("arena");},"","arena"),sourceButton(m.url,"Specifications"))));content.append(grid);
}

function comparisonCard(m:Model) {
  const b=benchmarkFor(view.cache,m);
  const details:[string,string][]=[
    ["Best suited to",m.bestFor],["Context window",m.context?`${(m.context/1000).toLocaleString()}K tokens`:"See provider"],
    ["API input / 1M",money(m.inputPrice)],["API output / 1M",money(m.outputPrice)],
    ["Measured intelligence",b?.intelligence?.toString()??"No matching snapshot"],["Output tokens / second",b?.speed?.toString()??"Not measured"],["Measured configuration",b?.name??"—"],
  ];
  return el("article",{class:"comparison-card"},el("div",{class:"meta"},mark(m.company),m.company),el("h2",{text:m.name}),...details.map(([k,v])=>el("div",{class:"comparison-row"},el("span",{text:k}),el("strong",{text:v}))),sourceButton(m.url,"Provider specifications"),el("p",{class:"fine-print",text:`API reference checked ${stamp(m.checked)}. ${m.note}`}));
}
function arena() {
  title("Two models. Your decision.","Compare what is published and measured, then try both on the same task.");
  const options=view.cache.models.map(m=>({value:m.id,label:m.name}));const l=view.cache.models.find(m=>m.id===left)!,r=view.cache.models.find(m=>m.id===right)!;
  content.append(el("div",{class:"versus-picker"},select("First model",options,left,v=>{left=v;render();}),el("span",{text:"VS"}),select("Second model",options,right,v=>{right=v;render();})),el("div",{class:"comparison-grid"},comparisonCard(l),comparisonCard(r)));
  const input=el("input",{type:"number",min:"0",max:"272000",step:"1000","aria-label":"Input tokens"});input.value=String(inputTokens);
  const output=el("input",{type:"number",min:"0",max:"128000",step:"1000","aria-label":"Output tokens"});output.value=String(outputTokens);
  const costs=el("div",{class:"cost-results"});
  const update=()=>{inputTokens=Math.max(0,Number(input.value)||0);outputTokens=Math.max(0,Number(output.value)||0);costs.replaceChildren(...[l,r].map(m=>el("div",{},el("span",{text:m.name}),el("strong",{text:m.inputPrice===null||m.outputPrice===null?"Not verified":money((inputTokens*m.inputPrice+outputTokens*m.outputPrice)/1000000)}))));};
  input.addEventListener("input",update);output.addEventListener("input",update);update();
  content.append(el("section",{class:"calculator"},sectionHeading("Estimate an API request","Standard short-context text pricing. Excludes caching, tools, taxes and reasoning variation."),el("div",{class:"calculator-inputs"},el("label",{},"Input tokens",input),el("label",{},"Output tokens",output)),costs),el("div",{class:"decision-note"},icon("research",20),el("p",{text:"For React or any specific project, there is no universal winner here. Run the same task with the same constraints and review correctness, time and total cost. Intelligence Index measures multiple tasks; it does not predict every coding task."}),sourceButton("https://artificialanalysis.ai/leaderboards/models","Benchmark source")));
}

function toolCard(t:Tool) {
  return el("article",{class:"tool-card"},el("div",{class:"tool-top"},mark(t.company),saveButton(`tool:${t.id}`)),btn(t.name,()=>toolDetail(t),"tool-title"),el("p",{text:t.bestFor}),el("div",{class:"tag-list"},...t.categories.slice(0,3).map(c=>pill(c))),el("div",{class:"tool-price"},t.freeTier?el("span",{class:"positive",text:"Free access"}):el("span",{text:"Paid / plan access"}),btn("View tool",()=>toolDetail(t),"text-link","arrow")));
}
function toolDetail(t:Tool) {
  const save=btn(view.preferences.saved.includes(`tool:${t.id}`)?"Saved":"Save tool",()=>void toggleSaved(`tool:${t.id}`).then(ok=>{if(ok){const selected=view.preferences.saved.includes(`tool:${t.id}`);save.replaceChildren(icon(selected?"check":"saved"),selected?"Saved":"Save tool");save.setAttribute("aria-label",selected?"Saved":"Save tool");}}),"","saved");
  openDialog(el("div",{class:"detail-content"},el("div",{class:"meta"},mark(t.company),t.company),el("h1",{text:t.name}),el("p",{class:"lead",text:t.bestFor}),el("p",{text:t.description}),el("h3",{text:"Price & access"}),el("p",{text:t.price}),el("p",{class:"fine-print",text:`Reference checked ${stamp(t.checked)}. Free access may have credits, limits or hardware costs. Check the provider before paying.`}),el("h3",{text:"Models"}),el("div",{class:"tag-list"},...t.models.map(m=>pill(m))),el("h3",{text:"Similar tools"}),el("div",{class:"similar-tools"},...t.similar.map(id=>view.cache.tools.find(x=>x.id===id)).filter((x):x is Tool=>!!x).map(other=>btn(other.name,()=>{closeDialog();toolDetail(other);},"","arrow"))),el("div",{class:"detail-actions"},btn("Visit tool",()=>source(t.url),"primary","external"),sourceButton(t.pricingUrl,"Current pricing"),save)),`${t.name} details`);
}
function toolsPage() {
  title("Explore AI","Start with what you want to do. Find a tool that fits.");
  const categoryGrid=el("div",{class:"category-grid"},...categories.map(c=>btn(c,()=>{toolCategory=c;render();},toolCategory===c?"category selected":"category",c.toLowerCase())));
  const input=el("input",{type:"search",placeholder:"Find a tool…","aria-label":"Search tools"});
  const result=el("div",{class:"tool-grid"});
  const update=()=>{
    const q=input.value.toLowerCase();const list=view.cache.tools.filter(t=>(toolCategory==="All"||t.categories.includes(toolCategory))&&(!freeOnly||t.freeTier)&&`${t.name} ${t.bestFor} ${t.description}`.toLowerCase().includes(q));
    result.replaceChildren(...(list.length?list.map(t=>toolCard(t)):[empty("No tools found","Try a different category or search.")]));
  };
  input.addEventListener("input",update);
  const free=el("input",{type:"checkbox"});free.checked=freeOnly;free.addEventListener("change",()=>{freeOnly=free.checked;update();});
  content.append(categoryGrid,el("div",{class:"tool-filters"},btn("All tools",()=>{toolCategory="All";render();},toolCategory==="All"?"selected":""),input,el("label",{class:"checkbox-label"},free,"Has free access")),result);update();
}

function ask(value:string) { if(!value.trim())return;query=value.trim().slice(0,500);jump("search"); }
function searchPage() {
  const result=search(view.cache,query);title(query,"Answers from Glowby's cached news, model references and tool directory.");
  const formInput=el("input",{type:"search",value:query,"aria-label":"Ask another AI question"});formInput.value=query;
  const form=el("form",{class:"ask-bar"},icon("search"),formInput,btn("Ask",()=>ask(formInput.value),"primary","arrow"));form.addEventListener("submit",e=>{e.preventDefault();ask(formInput.value);});content.append(form);
  if(result.newsIntent){
    content.append(sectionHeading(`What we found in ${result.period}`,`${result.articles.length} matching updates · last news check ${relative(view.cache.lastChecked)}`));
    if(!result.articles.length)content.append(empty("No sourced update found in that period","Refresh or broaden the question. Missing coverage doesn't mean nothing happened.",btn("Refresh sources",()=>void refresh(),"primary","refresh")));
    content.append(...result.articles.map(a=>articleRow(a)));return;
  }
  if(result.comparison&&result.models.length>=2){
    const first=result.models[0];const second=result.models.find(m=>m.company!==first.company)||result.models[1];
    content.append(sectionHeading("A useful starting comparison","Published capabilities and observed measurements, with exact configurations."),el("div",{class:"comparison-grid"},comparisonCard(first),comparisonCard(second)),btn("Open full comparison",()=>{left=first.id;right=second.id;jump("arena");},"primary","arena"));
    return;
  }
  if(result.tools.length){
    content.append(sectionHeading(result.free?"Tools with free access to consider":"Tools that match your question",result.free?"Free access often has limits. These are directory matches, not a tested quality ranking.":"Matched by the task, category and source descriptions."),el("div",{class:"tool-grid"},...result.tools.map(t=>toolCard(t))));
  }
  if(result.models.length)content.append(sectionHeading("Matching models"),...result.models.slice(0,3).map(m=>el("div",{class:"search-model"},mark(m.company),el("div",{},el("strong",{text:m.name}),el("p",{text:m.bestFor})),sourceButton(m.url,"Source"))));
  if(result.articles.length)content.append(sectionHeading("Related announcements"),...result.articles.slice(0,3).map(a=>articleRow(a)));
  if(!result.tools.length&&!result.models.length&&!result.articles.length)content.append(empty("No reliable match in this database","Try a company, task or model name. Glowby won't invent an answer without a source."));
  content.append(el("div",{class:"decision-note"},icon("check"),el("p",{text:"This search runs on your PC and uses the cached sources. It does not spend Claude or Codex tokens or send your question to a server."})));
}

function preferenceRow(label:string,detail:string,key:"enabled"|"background"|"alerts"|"daily") {
  const checkbox=el("input",{type:"checkbox",role:"switch","aria-label":label});checkbox.checked=view.preferences[key];
  checkbox.addEventListener("change",()=>void preferences({[key]:checkbox.checked}));
  return el("label",{class:"preference-row"},el("span",{},el("strong",{text:label}),el("small",{text:detail})),checkbox);
}
function settingsPage() {
  title("Make the signal yours","Choose what to follow, when to check, and how Glowby tells you.");
  const p=view.preferences;
  const companies=[...new Set(["OpenAI","Anthropic","Google","Meta","Hugging Face",...view.cache.articles.map(a=>a.company)])];
  const choices=(values:string[],selected:string[],key:"companies"|"families"|"topics")=>el("div",{class:"choice-grid"},...values.map(v=>{
    const c=el("input",{type:"checkbox"});c.checked=selected.includes(v);c.addEventListener("change",()=>void preferences({[key]:c.checked?[...selected,v]:selected.filter(x=>x!==v)}));return el("label",{class:"choice"},c,v);
  }));
  content.append(el("section",{class:"preferences-section"},sectionHeading("Reading & notifications"),preferenceRow("AI Pulse","Open the hub from Glowby's pet menu or tray.","enabled"),preferenceRow("Background checks","Opt in to public source requests while Glowby runs. No local work or questions are uploaded.","background"),preferenceRow("Watchlist alerts","Notify about new major announcements and observed benchmark changes after a baseline check.","alerts"),preferenceRow("Daily briefing","One notice per day after a background check. Never over a fullscreen game.","daily"),el("div",{class:"preference-row"},el("span",{},el("strong",{text:"Check interval"}),el("small",{text:"Only while background checks are enabled."})),select("Background check interval",[1,3,6,12,24].map(n=>({value:String(n),label:`Every ${n} hour${n===1?"":"s"}`})),String(p.everyHours),v=>void preferences({everyHours:Number(v)})))),
    el("section",{class:"preferences-section"},sectionHeading("Companies","Follow any combination. No selection means all companies."),choices(companies,p.companies,"companies"),sectionHeading("Model families","Optional narrowing by names mentioned in headlines."),choices(["GPT","Claude","Gemini","Llama","Grok"],p.families,"families"),sectionHeading("Topics","Used for your briefing and alerts. The full news feed stays available."),choices(topics.slice(1),p.topics,"topics")),
    el("section",{class:"preferences-section"},sectionHeading("Appearance"),filters(["system","light","dark"],p.theme,v=>void preferences({theme:v}))),
    el("section",{class:"preferences-section"},sectionHeading("Sources & freshness","A failed request keeps the previous material and its original date."),...view.cache.sources.map(s=>el("div",{class:"source-status"},el("span",{class:s.ok?"positive":"warning",text:s.ok?"●":"○"}),el("div",{},el("strong",{text:s.name}),el("small",{text:s.ok?`Checked ${relative(s.checked)}`:s.error})),sourceButton(s.url,"Open"))),!view.cache.sources.length?el("p",{class:"muted",text:"No live source check yet."}):null,btn("Refresh now",()=>void refresh(),"","refresh")),
    el("section",{class:"preferences-section"},sectionHeading("Your PC, your data"),el("p",{text:"News cache, follows and bookmarks live in Glowby's local data folder. The hub fetches public news feeds and Artificial Analysis's public benchmark table. No telemetry, project uploads, tracking images or AI account token access."}),el("p",{class:"fine-print",text:"Tool and API prices are a dated reference shipped with Glowby. Measured benchmark task costs refresh separately. Availability and plan details can change; the provider links are the authority."}),desktop?btn("Glowby companion settings",()=>void invoke("open_settings"),"","settings"):null));
}

let activeDialog:HTMLDialogElement|null=null;
function closeDialog() { activeDialog?.close();activeDialog?.remove();activeDialog=null; }
function openDialog(body:HTMLElement,label:string,paletteMode=false) {
  closeDialog();const dialog=el("dialog",{class:paletteMode?"command-dialog":"detail-dialog","aria-label":label});
  dialog.append(iconButton("Close","close",closeDialog),body);
  dialog.addEventListener("click",e=>{if(e.target===dialog)closeDialog();});dialog.addEventListener("close",()=>dialog.remove());document.body.append(dialog);activeDialog=dialog;dialog.showModal();return dialog;
}
function notifications() {
  const list=unreadArticles();const box=el("div",{class:"detail-content"},el("div",{class:"eyebrow",text:"YOUR WATCHLIST"}),el("h1",{text:"The latest signal"}),el("p",{text:`${list.length} unread updates. Public source checks ${view.preferences.background?"enabled":"run when you refresh"}.`}),btn("Mark all read",()=>{void preferences({read:[...new Set([...view.preferences.read,...list.map(a=>a.id)])]});closeDialog();},"quiet","check"),...list.slice(0,30).map(a=>articleRow(a)),!list.length?empty("You're caught up","Glowby will keep new watchlist updates here."):null);openDialog(box,"Watchlist updates");
}
function palette() {
  const input=el("input",{type:"search",placeholder:"Go somewhere, find a tool, or ask a question…","aria-label":"Command palette",autocomplete:"off"});
  const results=el("div",{class:"command-results",role:"listbox"});let selected=0;
  const update=()=>{
    const q=input.value.trim().toLowerCase();selected=0;
    const buttons=nav.filter(n=>!q||n.name.toLowerCase().includes(q)).map(n=>btn(n.name,()=>{closeDialog();jump(n.id);},"command-item",n.glyph));
    if(q){buttons.push(btn(`Ask Glowby: ${input.value}`,()=>{closeDialog();ask(input.value);},"command-item","sparkle"));buttons.push(...view.cache.tools.filter(t=>t.name.toLowerCase().includes(q)).slice(0,5).map(t=>btn(t.name,()=>{closeDialog();toolDetail(t);},"command-item","tools")));}
    results.replaceChildren(...buttons);highlight();
  };
  const highlight=()=>results.querySelectorAll<HTMLButtonElement>("button").forEach((b,i)=>{b.classList.toggle("selected",i===selected);b.setAttribute("aria-selected",String(i===selected));});
  input.addEventListener("input",update);input.addEventListener("keydown",e=>{const list=results.querySelectorAll<HTMLButtonElement>("button");if(e.key==="ArrowDown"||e.key==="ArrowUp"){e.preventDefault();selected=(selected+(e.key==="ArrowDown"?1:-1)+list.length)%list.length;highlight();list[selected]?.scrollIntoView({block:"nearest"});}if(e.key==="Enter"){e.preventDefault();list[selected]?.click();}});
  openDialog(el("div",{class:"command-content"},el("div",{class:"command-input"},icon("search"),input),results,el("div",{class:"command-footer"},"↑ ↓ to move",el("span",{text:"Enter to open · Esc to close"}))),"Search AI Pulse",true);update();input.focus();
}
function render() {
  sidebar.querySelectorAll<HTMLButtonElement>("[data-page]").forEach(b=>{b.classList.toggle("active",b.dataset.page===page);if(b.dataset.page===page)b.setAttribute("aria-current","page");else b.removeAttribute("aria-current");});
  breadcrumb.textContent=page==="search"?"Ask Glowby":page==="settings"?"Preferences":nav.find(n=>n.id===page)?.name??"AI Pulse";
  syncStatus.replaceChildren(el("span",{class:view.refreshing?"sync-dot busy":"sync-dot"}),view.refreshing?"Checking sources…":view.cache.lastChecked?`Checked ${stamp(view.cache.lastChecked)} ${new Date(view.cache.lastChecked).toLocaleTimeString(undefined,{hour:"2-digit",minute:"2-digit"})}`:"Sources not checked");
  unread.hidden=unreadArticles().length===0;
  content.replaceChildren();
  if(!desktop)content.append(el("div",{class:"preview-label",text:"Browser preview · local bookmarks · desktop alerts and live refresh run in Glowby"}));
  if(!view.preferences.enabled&&page!=="settings"){content.append(empty("AI Pulse is paused","Enable the hub in Preferences to read or refresh.",btn("Open Preferences",()=>jump("settings"),"primary")));return;}
  if(page==="home")home();else if(page==="news")news();else if(page==="saved")news(true);else if(page==="releases")news(false,true);else if(page==="models")modelsPage();else if(page==="arena")arena();else if(page==="tools")toolsPage();else if(page==="search")searchPage();else settingsPage();
  if(view.cache.sources.some(s=>!s.ok))content.append(el("div",{class:"partial-error"},icon("clock"),"Some sources could not be checked. Previous material keeps its original date. ",btn("View sources",()=>jump("settings"),"text-link")));
  const refreshButton=btn(view.refreshing?"Checking…":"Refresh sources",()=>void refresh(),"footer-refresh","refresh");refreshButton.disabled=view.refreshing;
  content.append(el("footer",{class:"page-footer"},el("span",{},"Fewer tabs. A clearer picture.",el("small",{text:"A personal AI desk, by Glowby."})),refreshButton));
}
async function refresh() {
  if(pending)return;pending=true;
  try{
    if(desktop){view.refreshing=true;render();view=await invoke<View>("pulse_refresh");}
    else{const res=await fetch("/src/pulse/preview-live.json");if(res.ok)view.cache=await res.json();else message("Live source refresh is available in the desktop app.");}
  }catch(e){message(String(e));view.refreshing=false;}finally{pending=false;render();}
}

document.addEventListener("keydown",e=>{
  if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==="k"){e.preventDefault();palette();}
  if(e.altKey&&!e.ctrlKey){const n=nav.find(n=>n.key===e.key);if(n){e.preventDefault();jump(n.id);}}
});
async function start() {
  shell();content.append(skeleton());
  try{
    if(desktop){view=await invoke<View>("pulse_view");await listen<View>("pulse://view",e=>{view=e.payload;applyTheme();render();});}
    else{
      try{const saved=localStorage.getItem("glowby-pulse-preview-preferences");if(saved)view.preferences={...defaultPreferences,...JSON.parse(saved)};}catch{/* defaults */}
      const res=await fetch("/src/pulse/preview-live.json");if(res.ok)view.cache=await res.json();
    }
    applyTheme();render();
    if(desktop&&view.preferences.enabled&&(Date.now()-Date.parse(view.cache.lastChecked||"1970-01-01")>6*3600000))void refresh();
  }catch(e){content.replaceChildren(empty("Couldn't open AI Pulse",String(e),btn("Try again",()=>void start(),"primary")));}
}
void start();
