import type { Article, Cache, Model, Tool } from "./types";

/** Retrieval is local and deterministic. Never invent an answer beyond the sources. */
export function normalize(text: string) { return text.toLowerCase().normalize("NFKD").replace(/[^a-z0-9\s.]/g, " "); }
const noise = new Set("what happened with this the a an in on of for to is are best new latest me about anything making i can which show tell please it was and vs compare free ai month week today models model tools".split(" "));
export function terms(query: string) {
  const text = normalize(query).replace(/\bgpt\b/g, "gpt openai").replace(/\bclaude\b/g, "claude anthropic").replace(/\bgemini\b/g, "gemini google").replace(/\breact\b/g, "react coding").replace(/\bvideos?\b/g, "video").replace(/\bimages?\b/g, "image").replace(/\bsongs?\b/g, "audio").replace(/\bslides?\b/g, "presentations");
  return [...new Set(text.split(/\s+/).filter(w => w.length > 1 && !noise.has(w)))];
}
function score(text: string, words: string[]) { const normalized = normalize(text); return words.reduce((n,w) => n + (normalized.includes(w) ? 1 : 0), 0); }
export interface Result { articles: Article[]; models: Model[]; tools: Tool[]; comparison: boolean; newsIntent: boolean; free: boolean; period: string; words: string[] }
export function search(cache: Cache, query: string, now = Date.now()): Result {
  const words = terms(query), lower = normalize(query);
  const free = /\bfree\b/.test(lower), newsIntent = /happened|news|released|updates|this week|this month|today/.test(lower);
  const comparison = /\bvs\b|versus|compare/.test(lower);
  const days = /week/.test(lower) ? 7 : /month/.test(lower) ? 30 : /today/.test(lower) ? 1 : 365;
  const filter = <T>(items: T[], text: (x:T)=>string) => items.map(x => ({x, score:score(text(x), words)})).filter(x => !words.length || x.score > 0).sort((a,b)=>b.score-a.score).map(x=>x.x);
  const articles = filter([...cache.articles, ...cache.changes].filter(a=> !newsIntent || (now-Date.parse(a.published) <= days*86400000 && Date.parse(a.published)<=now)), a=>`${a.title} ${a.company} ${a.tags.join(" ")} ${a.summary}`).slice(0, 8);
  const models = filter(cache.models, m=>`${m.name} ${m.company} ${m.bestFor} ${m.tags.join(" ")}`).slice(0, 5);
  const tools = filter(cache.tools.filter(t=>!free || t.freeTier), t=>`${t.name} ${t.company} ${t.bestFor} ${t.categories.join(" ")}`).slice(0, 6);
  return { articles, models, tools, comparison, newsIntent, free, words, period: days===7 ? "the last seven days" : days===30 ? "the last thirty days" : days===1 ? "the last 24 hours" : "the cached sources" };
}
export function benchmarkFor(cache: Cache, model: Model) {
  return cache.benchmarks.find(b=>normalize(b.name).startsWith(normalize(model.name)));
}
export function ranked(cache: Cache, metric: string, company = "All") {
  const field = metric === "Speed" ? "speed" : metric === "Cost per task" ? "cost" : "intelligence";
  const seen = new Set<string>();
  // One configuration per family name; retain the best measured configuration for this metric.
  return cache.benchmarks.filter(b => (company === "All" || b.company === company) && b[field] !== null).sort((a,b) => metric === "Cost per task" ? a[field]!-b[field]! : b[field]!-a[field]!).filter(b => {
    const family = b.name.split(" (")[0]; if (seen.has(family)) return false; seen.add(family); return true;
  });
}

/** Briefings favor useful announcements and source variety over one busy publisher. */
export function briefing(items: Article[], now = Date.now()) {
  const scored = items.filter(a => !a.tags.includes("Preview") && Number.isFinite(Date.parse(a.published)) && Date.parse(a.published)<=now)
    .map(a => ({a, age:Math.max(0,(now-Date.parse(a.published))/86400000)})).filter(x=>x.age<=14)
    .map(x=>({...x,score:(x.a.impact==="High"?12:0)+(x.a.tags.some(t=>t==="Coding"||t==="Agents")?5:0)+Math.max(0,14-x.age)}))
    .sort((a,b)=>b.score-a.score);
  const picked:Article[]=[], companies=new Set<string>();
  const repeatRelease = (a:Article) => a.kind === "release" && picked.some(p=>p.kind==="release"&&p.source===a.source);
  for(const {a} of scored){if(!repeatRelease(a)&&!companies.has(a.company)){picked.push(a);companies.add(a.company);}if(picked.length===5)break;}
  for(const {a} of scored){if(picked.length===5)break;if(!repeatRelease(a)&&!picked.some(p=>p.id===a.id))picked.push(a);}
  return picked;
}
