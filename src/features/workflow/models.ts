import type { AgentHarness, AgentProviderId } from "./types";

export type ModelOption = {
  id: string;
  label: string;
  description: string;
  baseId?: string;
  fastVariantId?: string;
  isFastVariant?: boolean;
  supportsFastToggle?: boolean;
};

export function modelOptionForValue(
  catalog: ProviderModels,
  model: string,
): ModelOption | undefined {
  return catalog.models.find(
    (option) =>
      option.id === model ||
      option.baseId === model ||
      option.fastVariantId === model,
  );
}

export function supportsFastToggle(
  option: ModelOption | undefined,
): option is ModelOption & { baseId: string; fastVariantId: string } {
  return Boolean(
    option?.supportsFastToggle === true &&
      option.baseId &&
      option.fastVariantId,
  );
}

export function isFastModel(
  option: ModelOption | undefined,
  model: string,
): boolean {
  return supportsFastToggle(option) && option?.fastVariantId === model;
}

export function modelIdForFastToggle(
  option: ModelOption | undefined,
  fast: boolean,
): string | null {
  if (!supportsFastToggle(option)) return null;
  return fast ? (option.fastVariantId ?? null) : (option.baseId ?? null);
}

export type ProviderModels = {
  provider: AgentProviderId;
  harness: AgentHarness;
  defaultModel: string;
  models: ModelOption[];
  allowCustom: boolean;
  source?: "discovered" | "fallback" | string;
  available?: boolean;
  error?: string | null;
  requiresAccount: boolean;
  supportsOAuth: boolean;
  supportsApiKey: boolean;
  supportsUsage: boolean;
  accountConnected: boolean;
  nativeRuntimeAvailable: boolean;
};

const CLI_MODEL_CAPABILITIES = {
  harness: "cli" as const,
  requiresAccount: false,
  supportsOAuth: false,
  supportsApiKey: false,
  supportsUsage: false,
  accountConnected: false,
  nativeRuntimeAvailable: false,
};

/** Fallback catalog if the backend isn't available yet. */
export const FALLBACK_PROVIDER_MODELS: ProviderModels[] = [
  {
    provider: "claude_code",
    ...CLI_MODEL_CAPABILITIES,
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
    ...CLI_MODEL_CAPABILITIES,
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
    ...CLI_MODEL_CAPABILITIES,
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
    ...CLI_MODEL_CAPABILITIES,
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
    ...CLI_MODEL_CAPABILITIES,
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
    ...CLI_MODEL_CAPABILITIES,
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
    ...CLI_MODEL_CAPABILITIES,
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
  {
    provider: "pi",
    ...CLI_MODEL_CAPABILITIES,
    defaultModel: "default",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      { id: "default", label: "CLI default", description: "" },
      {
        id: "anthropic/claude-sonnet-5",
        label: "Claude Sonnet 5",
        description: "",
      },
      { id: "openai/gpt-5.6-luna", label: "GPT-5.6 Luna", description: "" },
      {
        id: "google/gemini-3-pro-preview",
        label: "Gemini 3 Pro Preview",
        description: "",
      },
    ],
  },
  {
    provider: "omp",
    ...CLI_MODEL_CAPABILITIES,
    defaultModel: "default",
    allowCustom: true,
    source: "fallback",
    available: false,
    models: [
      { id: "default", label: "CLI default", description: "" },
      {
        id: "anthropic/claude-sonnet-5",
        label: "Claude Sonnet 5",
        description: "",
      },
      { id: "openai/gpt-5.6-luna", label: "GPT-5.6 Luna", description: "" },
      {
        id: "google/gemini-3-pro-preview",
        label: "Gemini 3 Pro Preview",
        description: "",
      },
    ],
  },
];

export const NATIVE_UNAVAILABLE_PROVIDER_MODELS: ProviderModels[] =
  FALLBACK_PROVIDER_MODELS.map((catalog) => ({
    provider: catalog.provider,
    harness: "alfred",
    defaultModel: "",
    models: [],
    allowCustom: false,
    source: "unavailable",
    available: false,
    error: "native_runtime_unavailable",
    requiresAccount: true,
    supportsOAuth: false,
    supportsApiKey: false,
    supportsUsage: false,
    accountConnected: false,
    nativeRuntimeAvailable: false,
  }));

export function modelsForProvider(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
  harness: AgentHarness = "cli",
): ProviderModels {
  return (
    catalogs.find(
      (catalog) =>
        catalog.provider === provider && (catalog.harness ?? "cli") === harness,
    ) ??
    (harness === "cli"
      ? FALLBACK_PROVIDER_MODELS.find((c) => c.provider === provider)
      : NATIVE_UNAVAILABLE_PROVIDER_MODELS.find((c) => c.provider === provider)) ??
    FALLBACK_PROVIDER_MODELS[0]
  );
}

export function defaultModelFor(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
  harness: AgentHarness = "cli",
): string {
  return modelsForProvider(catalogs, provider, harness).defaultModel;
}

export function selectionForAgentTarget(
  catalogs: ProviderModels[],
  provider: AgentProviderId,
  harness: AgentHarness,
  currentModel: string | null | undefined,
): { harness: AgentHarness; model: string | null; accountRef: null } {
  const catalog = modelsForProvider(catalogs, provider, harness);
  const normalizedModel = currentModel?.trim() || null;
  const compatible = normalizedModel
    ? Boolean(modelOptionForValue(catalog, normalizedModel))
    : false;
  return {
    harness,
    model: compatible ? normalizedModel : catalog.defaultModel || null,
    accountRef: null,
  };
}
