export interface Article {
  id: string; title: string; company: string; source: string; url: string;
  published: string; summary: string; why: string; tags: string[]; impact: string;
  kind: string; checked: string;
}
export interface Model {
  id: string; name: string; company: string; bestFor: string; inputPrice: number | null;
  outputPrice: number | null; context: number | null; url: string; checked: string;
  released: string; tags: string[]; note: string;
}
export interface Tool {
  id: string; name: string; company: string; bestFor: string; description: string;
  price: string; freeTier: boolean; categories: string[]; models: string[];
  similar: string[]; url: string; pricingUrl: string; checked: string;
}
export interface Benchmark {
  name: string; company: string; context: string; intelligence: number | null;
  cost: number | null; speed: number | null; latency: number | null; rank: number;
  movement: number | null; checked: string; url: string;
}
export interface SourceStatus { name: string; url: string; checked: string; ok: boolean; error: string }
export interface Cache {
  articles: Article[]; models: Model[]; tools: Tool[]; benchmarks: Benchmark[];
  sources: SourceStatus[]; lastChecked: string; changes: Article[]; lastAttempt: number;
}
export interface Preferences {
  enabled: boolean; background: boolean; alerts: boolean; daily: boolean; everyHours: number;
  companies: string[]; families: string[]; topics: string[]; saved: string[]; read: string[];
  theme: string; lastBriefingDay: string;
}
export interface View { cache: Cache; preferences: Preferences; refreshing: boolean }
export type Page = "home" | "news" | "models" | "arena" | "tools" | "releases" | "saved" | "settings" | "search";
export const categories = ["Coding", "Image", "Video", "Audio", "Study", "Research", "Data", "Writing", "Agents", "Presentations"];
export const topics = ["All", "Models", "Agents", "Coding", "Image", "Video", "Audio", "Research", "Business"];

export const defaultPreferences: Preferences = {
  enabled: true, background: false, alerts: true, daily: true, everyHours: 6,
  companies: ["OpenAI", "Anthropic", "Google"], families: [], topics: ["Models", "Coding", "Agents"],
  saved: [], read: [], theme: "system", lastBriefingDay: "",
};
