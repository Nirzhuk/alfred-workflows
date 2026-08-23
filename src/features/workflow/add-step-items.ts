import type { AgentProviderId, WorkflowNodeData } from "./types";
import {
  defaultCustomAgentNodeData,
  defaultAppActionNodeData,
  defaultFileInjectNodeData,
  defaultGitHostNodeData,
  defaultGitStatusNodeData,
  defaultHttpNodeData,
  defaultNotifyNodeData,
  defaultScriptNodeData,
  defaultTemplateNodeData,
  defaultWriteFileNodeData,
} from "./types";

export type AddStepItem =
  | { kind: "prompt"; label: string }
  | { kind: "choose"; label: string }
  | { kind: "memory"; label: string }
  | { kind: "agent"; label: string; provider: AgentProviderId }
  | {
      kind: "step";
      label: string;
      /** React Flow node `type` id. */
      type: string;
      data: WorkflowNodeData;
    };

export type AddStepGroupId = "context" | "agent" | "apps" | "sink";

export type AddStepGroup = {
  id: AddStepGroupId;
  label: string;
  items: AddStepItem[];
};

/** Categorized palette for canvas “Add step” menus. */
export const ADD_STEP_GROUPS: AddStepGroup[] = [
  {
    id: "context",
    label: "Context",
    items: [
      { kind: "prompt", label: "Input" },
      { kind: "memory", label: "Memories" },
      {
        kind: "step",
        label: "Template",
        type: "template",
        data: defaultTemplateNodeData(),
      },
      {
        kind: "step",
        label: "File inject",
        type: "fileInject",
        data: defaultFileInjectNodeData(),
      },
      {
        kind: "step",
        label: "Git status",
        type: "gitStatus",
        data: defaultGitStatusNodeData(),
      },
    ],
  },
  {
    id: "agent",
    label: "Agent",
    items: [
      { kind: "agent", label: "Claude Code", provider: "claude_code" },
      { kind: "agent", label: "Cursor", provider: "cursor" },
      { kind: "agent", label: "Codex", provider: "codex" },
      { kind: "agent", label: "OpenCode", provider: "opencode" },
      {
        kind: "agent",
        label: "GitHub Copilot",
        provider: "github_copilot",
      },
      { kind: "agent", label: "Gemini", provider: "gemini" },
      { kind: "agent", label: "Grok", provider: "grok" },
      {
        kind: "step",
        label: "Custom",
        type: "customAgent",
        data: defaultCustomAgentNodeData(),
      },
      {
        kind: "step",
        label: "Script",
        type: "script",
        data: defaultScriptNodeData(),
      },
      {
        kind: "step",
        label: "HTTP",
        type: "http",
        data: defaultHttpNodeData(),
      },
      {
        kind: "step",
        label: "Notify",
        type: "notify",
        data: defaultNotifyNodeData(),
      },
    ],
  },
  {
    id: "apps",
    label: "Apps",
    items: [
      {
        kind: "step",
        label: "App action",
        type: "appAction",
        data: defaultAppActionNodeData(),
      },
    ],
  },
  {
    id: "sink",
    label: "Sink",
    items: [
      { kind: "choose", label: "Output" },
      {
        kind: "step",
        label: "Write file",
        type: "writeFile",
        data: defaultWriteFileNodeData(),
      },
      {
        kind: "step",
        label: "Create PR",
        type: "gitHost",
        data: defaultGitHostNodeData("Create PR", "pr"),
      },
      {
        kind: "step",
        label: "Open issue",
        type: "gitHost",
        data: defaultGitHostNodeData("Open issue", "issue"),
      },
    ],
  },
];

/** Flat list derived from groups (legacy / callers that don’t need categories). */
export const ADD_STEP_ITEMS: AddStepItem[] = ADD_STEP_GROUPS.flatMap(
  (group) => group.items,
);
