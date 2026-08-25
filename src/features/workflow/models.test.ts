import { describe, expect, test } from "bun:test";
import {
  isFastModel,
  modelIdForFastToggle,
  modelOptionForValue,
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
  defaultModel: pairedModel.id,
  allowCustom: true,
  models: [pairedModel],
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
