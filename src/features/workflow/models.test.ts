import { describe, expect, test } from "bun:test";
import {
  isFastModel,
  modelIdForFastToggle,
  modelOptionForValue,
  modelsForProvider,
  selectionForAgentTarget,
  supportsFastToggle,
  type ModelOption,
  type ProviderModels,
} from "./models";

const pairedModel: ModelOption = {
  id: "cursor-grok-4.5-high",
  label: "Cursor Grok 4.5 High",
  description: "Cursor",
  baseId: "cursor-grok-4.5-high",
  fastVariantId: "cursor-grok-4.5-high-fast",
  supportsFastToggle: true,
};

const catalog: ProviderModels = {
  provider: "cursor",
  harness: "cli",
  defaultModel: pairedModel.id,
  allowCustom: true,
  models: [pairedModel],
  requiresAccount: false,
  supportsOAuth: false,
  supportsApiKey: false,
  supportsUsage: false,
  accountConnected: false,
  nativeRuntimeAvailable: false,
};

describe("fast model picker state", () => {
  test("shows Fast only for a confident pair", () => {
    expect(supportsFastToggle(pairedModel)).toBe(true);
    expect(
      supportsFastToggle({
        id: "cursor-grok-4.5-high",
        label: "Cursor Grok 4.5 High",
        description: "Cursor",
      }),
    ).toBe(false);
  });

  test("swaps the stored concrete model id", () => {
    expect(modelIdForFastToggle(pairedModel, true)).toBe(
      "cursor-grok-4.5-high-fast",
    );
    expect(modelIdForFastToggle(pairedModel, false)).toBe(
      "cursor-grok-4.5-high",
    );
    expect(modelIdForFastToggle(undefined, true)).toBeNull();
  });

  test("restores Fast when a workflow stores the fast id", () => {
    const selected = modelOptionForValue(
      catalog,
      "cursor-grok-4.5-high-fast",
    );

    expect(selected?.id).toBe("cursor-grok-4.5-high");
    expect(isFastModel(selected, "cursor-grok-4.5-high-fast")).toBe(true);
    expect(isFastModel(selected, "cursor-grok-4.5-high")).toBe(false);
    expect(modelOptionForValue(catalog, "my-custom-model")).toBeUndefined();
  });
});

describe("agent harness model boundaries", () => {
  const nativeCatalog: ProviderModels = {
    provider: "cursor",
    harness: "alfred",
    defaultModel: "",
    allowCustom: false,
    models: [],
    available: false,
    error: "native_runtime_unavailable",
    requiresAccount: true,
    supportsOAuth: false,
    supportsApiKey: false,
    supportsUsage: false,
    accountConnected: false,
    nativeRuntimeAvailable: false,
  };

  test("scopes catalogs to provider and harness", () => {
    expect(modelsForProvider([catalog, nativeCatalog], "cursor", "cli")).toBe(
      catalog,
    );
    expect(
      modelsForProvider([catalog, nativeCatalog], "cursor", "alfred"),
    ).toBe(nativeCatalog);
  });

  test("clears only incompatible target fields when switching", () => {
    expect(
      selectionForAgentTarget(
        [catalog, nativeCatalog],
        "cursor",
        "alfred",
        pairedModel.id,
      ),
    ).toEqual({ harness: "alfred", model: null, accountRef: null });
    expect(
      selectionForAgentTarget(
        [catalog, nativeCatalog],
        "cursor",
        "cli",
        pairedModel.id,
      ),
    ).toEqual({
      harness: "cli",
      model: pairedModel.id,
      accountRef: null,
    });

    const compatibleNative = {
      ...nativeCatalog,
      models: [pairedModel],
      defaultModel: pairedModel.id,
    };
    expect(
      selectionForAgentTarget(
        [catalog, compatibleNative],
        "cursor",
        "alfred",
        pairedModel.id,
      ),
    ).toEqual({
      harness: "alfred",
      model: pairedModel.id,
      accountRef: null,
    });
  });
});
