import { describe, expect, test } from "bun:test";
import { ADD_STEP_ITEMS } from "../src/features/workflow/add-step-items";
import {
  DEFAULT_SCRIPT_MESSAGE,
  defaultInputScript,
  defaultScriptNodeData,
  isPromptNodeData,
  isScriptNodeData,
  isShellNodeData,
  titleForNodeType,
  type PromptNodeData,
} from "../src/features/workflow/types";

describe("defaultScriptNodeData", () => {
  test("starts as an inline script that appends its output", () => {
    const data = defaultScriptNodeData();
    expect(data.kind).toBe("script");
    expect(data.label).toBe("Script");
    expect(data.source).toBe("inline");
    expect(data.path).toBe("");
    expect(data.body).toBe("");
    expect(data.interpreter).toBeTruthy();
    expect(data.appendOutput).toBe(true);
  });

  test("is recognized as a script and never as a shell node", () => {
    const data = defaultScriptNodeData();
    expect(isScriptNodeData(data)).toBe(true);
    expect(isShellNodeData(data)).toBe(false);
  });
});

describe("defaultInputScript", () => {
  test("defaults to instruct-only against a file", () => {
    const script = defaultInputScript();
    expect(script.source).toBe("file");
    expect(script.run).toBe(false);
    expect(script.message).toBe(DEFAULT_SCRIPT_MESSAGE);
  });

  test("honors an explicit source", () => {
    expect(defaultInputScript("inline").source).toBe("inline");
  });
});

describe("Input node script field", () => {
  test("is optional, so legacy graphs still read as Input nodes", () => {
    const legacy: PromptNodeData = { label: "Input", prompt: "do the thing" };
    expect(isPromptNodeData(legacy)).toBe(true);
    expect(legacy.script).toBeUndefined();
  });

  test("clearing the script drops the key from persisted JSON", () => {
    const withScript: PromptNodeData = {
      label: "Input",
      prompt: "do the thing",
      script: defaultInputScript(),
    };
    const cleared = { ...withScript, script: undefined };
    expect(JSON.parse(JSON.stringify(cleared))).not.toHaveProperty("script");
  });
});

describe("add step palette", () => {
  test("offers Script and no longer offers Shell", () => {
    const types = ADD_STEP_ITEMS.map((item) =>
      item.kind === "step" ? item.type : item.kind,
    );
    expect(types).toContain("script");
    expect(types).not.toContain("shell");
  });
});

describe("titleForNodeType", () => {
  test("names script nodes, and keeps naming legacy shell nodes", () => {
    expect(titleForNodeType("script")).toBe("Script");
    expect(titleForNodeType("shell")).toBe("Shell");
  });
});
