import type { AgentProviderId } from "./types";

export type ModelOption = {
  id: string;
  label: string;
  description: string;
};

export type ProviderModels = {
  provider: AgentProviderId;
  defaultModel: string;
  models: ModelOption[];
  allowCustom: boolean;
  source?: "discovered" | "fallback" | string;
  available?: boolean;
  error?: string | null;
};

/** Fallback catalog if the backend isn't available yet. */
export const FALLBACK_PROVIDER_MODELS: ProviderModels[] = [
  {
    provider: "claude_code",
    defaultModel: "sonnet",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      { id: "sonnet", label: "Sonnet 5", description: "" },
      { id: "opus", label: "Opus 5", description: "" },
      { id: "haiku", label: "Haiku 4.5", description: "" },
      { id: "fable", label: "Fable 5", description: "" },
    ],
  },
  {
    provider: "cursor",
    defaultModel: "grok-4.5",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      { id: "default", label: "Auto", description: "" },
      { id: "grok-4.5", label: "Cursor Grok 4.5", description: "" },
      { id: "composer-2.5", label: "Composer 2.5", description: "" },
      { id: "claude-opus-5", label: "Opus 5", description: "" },
      { id: "claude-sonnet-5", label: "Sonnet 5", description: "" },
      { id: "claude-fable-5", label: "Fable 5", description: "" },
      { id: "gpt-5.6-sol", label: "GPT-5.6 Sol", description: "" },
      { id: "gpt-5.6-terra", label: "GPT-5.6 Terra", description: "" },
      { id: "gpt-5.3-codex", label: "Codex 5.3", description: "" },
      { id: "gemini-3.1-pro", label: "Gemini 3.1 Pro", description: "" },
    ],
  },
  {
    provider: "codex",
    defaultModel: "gpt-5.6-luna",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      { id: "gpt-5.6-luna", label: "gpt-5.6-luna", description: "" },
      { id: "gpt-5.6-terra", label: "gpt-5.6-terra", description: "" },
      { id: "gpt-5.6-sol", label: "gpt-5.6-sol", description: "" },
    ],
  },
  {
    provider: "opencode",
    defaultModel: "opencode/big-pickle",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      {
        id: "opencode/big-pickle",
        label: "opencode/big-pickle",
        description: "",
      },
    ],
  },
  {
    provider: "github_copilot",
    defaultModel: "claude-sonnet-4.5",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      {
        id: "claude-sonnet-4.5",
        label: "Claude Sonnet 4.5",
        description: "GitHub Copilot default",
      },
      { id: "claude-opus-4.5", label: "Claude Opus 4.5", description: "" },
      {
        id: "claude-haiku-4.5",
        label: "Claude Haiku 4.5",
        description: "",
      },
      { id: "gpt-5.3-codex", label: "GPT-5.3 Codex", description: "" },
      { id: "gpt-5.2", label: "GPT-5.2", description: "" },
    ],
  },
  {
    provider: "gemini",
    defaultModel: "auto",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      { id: "auto", label: "Auto", description: "Gemini CLI model routing" },
      {
        id: "gemini-3.1-pro-preview",
        label: "Gemini 3.1 Pro Preview",
        description: "",
      },
      {
        id: "gemini-3-pro-preview",
        label: "Gemini 3 Pro Preview",
        description: "",
      },
      {
        id: "gemini-3-flash-preview",
        label: "Gemini 3 Flash Preview",
        description: "",
      },
      { id: "gemini-2.5-pro", label: "Gemini 2.5 Pro", description: "" },
      {
        id: "gemini-2.5-flash",
        label: "Gemini 2.5 Flash",
        description: "",
      },
    ],
  },
  {
    provider: "grok",
    defaultModel: "grok-build",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      { id: "grok-build", label: "Grok Build", description: "" },
      { id: "grok-4.5", label: "Grok 4.5", description: "" },
      { id: "grok-code-fast-1", label: "Grok Code Fast 1", description: "" },
    ],
  },
];

export function modelsForProvider(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
): ProviderModels {
  return (
    catalogs.find((c) => c.provider === provider) ??
    FALLBACK_PROVIDER_MODELS.find((c) => c.provider === provider) ??
    FALLBACK_PROVIDER_MODELS[0]
  );
}

export function defaultModelFor(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
): string {
  return modelsForProvider(catalogs, provider).defaultModel;
}
