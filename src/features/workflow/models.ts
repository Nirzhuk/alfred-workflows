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
      { id: "sonnet", label: "sonnet", description: "" },
      { id: "opus", label: "opus", description: "" },
      { id: "haiku", label: "haiku", description: "" },
      { id: "fable", label: "fable", description: "" },
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
