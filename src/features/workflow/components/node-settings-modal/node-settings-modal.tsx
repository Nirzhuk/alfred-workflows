import { useEffect, useMemo, useState } from "react";
import { Icon } from "../../../../components/icon";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  MenuItem,
} from "../../../../components/menu";
import { Modal, ModalHeader } from "../../../../components/modal";
import { AppActionSettings } from "../../../integrations/app-action-settings";
import * as api from "../../api";
import {
  modelsForProvider,
  type ProviderModels,
} from "../../models";
import { useWorkflowStore } from "../../store";
import {
  mergeAttachments,
  pickFileAttachments,
  pickFolderAttachments,
} from "../../attachments";
import { SelectControl } from "../../../../components/select-control";
import {
  agentSkillNames,
  DEFAULT_SCRIPT_MESSAGE,
  defaultInputScript,
  isAppActionNodeData,
  isAgentNodeData,
  isCustomAgentNodeData,
  isFileInjectNodeData,
  isGitHostNodeData,
  isGitStatusNodeData,
  isHttpNodeData,
  isMemoryNodeData,
  isNotifyNodeData,
  isOutputNodeData,
  isPromptNodeData,
  isScriptNodeData,
  isShellNodeData,
  isTemplateNodeData,
  isWriteFileNodeData,
  titleForNodeType,
  type AgentProviderId,
  type InputAttachment,
  type InputScript,
  type OutputMemory,
  type OutputNodeData,
  type PromptNodeData,
  type ScriptRef,
  type Skill,
} from "../../types";
import { InputAttachmentList } from "../input-attachment-list";
import { agentLabel } from "../agent-mark";
import { SkillPicker } from "../skill-picker";
import {
  CustomAgentSettings,
  FileInjectSettings,
  GitHostSettings,
  GitStatusSettings,
  HttpSettings,
  NotifySettings,
  ScriptRefFields,
  ScriptSettings,
  ShellSettings,
  TemplateSettings,
  WriteFileSettings,
} from "./utility-settings";

type Props = {
  nodeId: string;
  onClose: () => void;
};

export function NodeSettingsModal({ nodeId, onClose }: Props) {
  const nodes = useWorkflowStore((s) => s.nodes);
  const skills = useWorkflowStore((s) => s.skills);
  const providerModels = useWorkflowStore((s) => s.providerModels);
  const memories = useWorkflowStore((s) => s.memories);
  const activeWorkflowId = useWorkflowStore((s) => s.activeWorkflowId);
  const updateNodeData = useWorkflowStore((s) => s.updateNodeData);
  const loadProviderModels = useWorkflowStore((s) => s.loadProviderModels);
  const linkMemory = useWorkflowStore((s) => s.linkMemory);

  const node = useMemo(
    () => nodes.find((n) => n.id === nodeId) ?? null,
    [nodes, nodeId],
  );

  const [linkable, setLinkable] = useState<OutputMemory[]>([]);
  const [linkQuery, setLinkQuery] = useState("");

  useEffect(() => {
    if (!node) onClose();
  }, [node, onClose]);

  useEffect(() => {
    if (!node || !isMemoryNodeData(node.data) || !activeWorkflowId) {
      setLinkable([]);
      return;
    }
    let cancelled = false;
    void api.listLinkableMemories(activeWorkflowId).then((rows) => {
      if (!cancelled) setLinkable(rows);
    });
    return () => {
      cancelled = true;
    };
  }, [
    node?.id,
    node && isMemoryNodeData(node.data) ? node.data.memoryIds.join(",") : "",
    activeWorkflowId,
    memories.length,
  ]);

  if (!node) return null;

  const heading = titleForNodeType(node.type);

  return (
    <Modal
      size="settings"
      onClose={onClose}
      labelledBy="node-settings-title"
      describedBy="node-settings-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="sliders" size={20} />
          </span>
        }
        title={`${heading} settings`}
        titleId="node-settings-title"
        description="Configure how this node behaves when the workflow runs."
        descriptionId="node-settings-description"
        actions={
          <button type="button" className="ghost" onClick={onClose}>
            Done
          </button>
        }
      />

      <div className="node-settings-modal-body">
        {isPromptNodeData(node.data) ? (
          <InputSettings
            label={node.data.label}
            prompt={node.data.prompt}
            blocked={Boolean(node.data.blocked)}
            attachments={node.data.attachments ?? []}
            script={node.data.script}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isMemoryNodeData(node.data) ? (
          <MemorySettings
            label={node.data.label}
            memoryIds={node.data.memoryIds ?? []}
            memories={memories}
            linkable={linkable}
            linkQuery={linkQuery}
            activeWorkflowId={activeWorkflowId}
            onLinkQueryChange={setLinkQuery}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
            onLinkMemory={linkMemory}
          />
        ) : null}

        {isAgentNodeData(node.data) ? (
          <AgentSettings
            key={`${node.id}:${node.data.provider}`}
            provider={node.data.provider}
            model={node.data.model}
            skillNames={agentSkillNames(node.data)}
            skills={skills}
            providerModels={providerModels}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
            onRefreshModels={() => void loadProviderModels()}
          />
        ) : null}

        {isCustomAgentNodeData(node.data) ? (
          <CustomAgentSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isAppActionNodeData(node.data) ? (
          <AppActionSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {node.type === "chooseOutput" && isOutputNodeData(node.data) ? (
          <OutputSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isTemplateNodeData(node.data) ? (
          <TemplateSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isFileInjectNodeData(node.data) ? (
          <FileInjectSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isGitStatusNodeData(node.data) ? (
          <GitStatusSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isShellNodeData(node.data) ? (
          <ShellSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isScriptNodeData(node.data) ? (
          <ScriptSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isHttpNodeData(node.data) ? (
          <HttpSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isNotifyNodeData(node.data) ? (
          <NotifySettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isWriteFileNodeData(node.data) ? (
          <WriteFileSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}

        {isGitHostNodeData(node.data) ? (
          <GitHostSettings
            data={node.data}
            onUpdate={(patch) => updateNodeData(node.id, patch)}
          />
        ) : null}
      </div>
    </Modal>
  );
}

type InputSettingsProps = {
  label: string;
  prompt: string;
  blocked: boolean;
  attachments: InputAttachment[];
  script?: InputScript;
  onUpdate: (patch: Partial<PromptNodeData>) => void;
};

function InputSettings({
  label,
  prompt,
  blocked,
  attachments,
  script,
  onUpdate,
}: InputSettingsProps) {
  const addFiles = async () => {
    try {
      const picked = await pickFileAttachments();
      if (picked.length === 0) return;
      onUpdate({ attachments: mergeAttachments(attachments, picked) });
    } catch (e) {
      console.warn("File picker unavailable", e);
    }
  };

  const addFolders = async () => {
    try {
      const picked = await pickFolderAttachments();
      if (picked.length === 0) return;
      onUpdate({ attachments: mergeAttachments(attachments, picked) });
    } catch (e) {
      console.warn("Folder picker unavailable", e);
    }
  };

  const removeAttachment = (attachmentId: string) => {
    onUpdate({
      attachments: attachments.filter((a) => a.id !== attachmentId),
    });
  };

  return (
    <>
      <div className={`wf-input-block-setting${blocked ? " is-blocked" : ""}`}>
        <div className="wf-input-block-setting-copy">
          <strong>{blocked ? "Editing is blocked" : "Block input editing"}</strong>
          <span>
            {blocked
              ? "Unblock this Input before changing its prompt, label, attachments, or placement."
              : "Protect this Input from accidental changes on the canvas or in settings."}
          </span>
        </div>
        <button
          type="button"
          className={blocked ? "primary" : "ghost"}
          onClick={() => onUpdate({ blocked: !blocked })}
        >
          {blocked ? "Unblock" : "Block"}
        </button>
      </div>
      <label className="field">
        <span>Label</span>
        <input
          type="text"
          value={label}
          disabled={blocked}
          onChange={(e) => onUpdate({ label: e.target.value })}
        />
      </label>
      <label className="field">
        <span>Prompt</span>
        <textarea
          value={prompt}
          rows={10}
          placeholder="What should the agent do?"
          disabled={blocked}
          onChange={(e) => onUpdate({ prompt: e.target.value })}
        />
      </label>
      <div className="field wf-attach-field">
        <div className="wf-attach-field-header">
          <span>Files &amp; folders</span>
          <div className="wf-input-attach-actions">
            <button
              type="button"
              className="ghost wf-input-attach-btn"
              disabled={blocked}
              onClick={() => void addFiles()}
            >
              + File
            </button>
            <button
              type="button"
              className="ghost wf-input-attach-btn"
              disabled={blocked}
              onClick={() => void addFolders()}
            >
              + Folder
            </button>
          </div>
        </div>
        <p className="hint">
          Attached paths are included in the run context so the agent can use
          them when building the task.
        </p>
        <InputAttachmentList
          attachments={attachments}
          onRemove={removeAttachment}
          readOnly={blocked}
          variant="modal"
        />
      </div>
      <div className="field wf-attach-field">
        <div className="wf-attach-field-header">
          <span>Script</span>
        </div>
        <label className="field">
          <span>Use a script</span>
          <SelectControl
            value={script ? script.source : "none"}
            disabled={blocked}
            onChange={(e) => {
              const next = e.target.value;
              if (next === "none") {
                onUpdate({ script: undefined });
                return;
              }
              const source = next as ScriptRef["source"];
              onUpdate({
                script: script
                  ? { ...script, source }
                  : defaultInputScript(source),
              });
            }}
          >
            <option value="none">No script</option>
            <option value="file">File on disk</option>
            <option value="inline">Saved script</option>
          </SelectControl>
        </label>
        {script ? (
          <>
            <ScriptRefFields
              data={script}
              disabled={blocked}
              onUpdate={(patch) => onUpdate({ script: { ...script, ...patch } })}
            />
            <label className="field">
              <span>Message</span>
              <textarea
                rows={2}
                value={script.message}
                disabled={blocked}
                placeholder={DEFAULT_SCRIPT_MESSAGE}
                onChange={(e) =>
                  onUpdate({ script: { ...script, message: e.target.value } })
                }
              />
            </label>
            <label className="field checkbox-field">
              <input
                type="checkbox"
                checked={script.run}
                disabled={blocked}
                onChange={(e) =>
                  onUpdate({ script: { ...script, run: e.target.checked } })
                }
              />
              <span>Run it before the agent</span>
            </label>
            <p className="hint">
              By default the agent is only told about the script and decides
              when to run it. When run here, the output is appended under “Script
              output” — a non-zero exit does not stop the run, the agent gets the
              failure instead.
            </p>
          </>
        ) : null}
      </div>
    </>
  );
}

type OutputSettingsProps = {
  data: OutputNodeData;
  onUpdate: (patch: Partial<OutputNodeData>) => void;
};

function OutputSettings({ data, onUpdate }: OutputSettingsProps) {
  return (
    <>
      <label className="field">
        <span>Label</span>
        <input
          type="text"
          value={data.label}
          onChange={(e) => onUpdate({ label: e.target.value })}
        />
      </label>
      <p className="hint">
        Captures the upstream agent result. Choose whether to save it as a
        memory, publish it as the run’s final output, format it as HTML, and
        include changed files.
      </p>
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.htmlReport}
          onChange={(e) => onUpdate({ htmlReport: e.target.checked })}
        />
        <span>Request an HTML report</span>
      </label>
      {data.htmlReport ? (
        <p className="hint">
          The nearest connected upstream agent will be asked for a complete,
          self-contained HTML document.
        </p>
      ) : null}
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.saveToMemory}
          onChange={(e) => onUpdate({ saveToMemory: e.target.checked })}
        />
        <span>Save to memories</span>
      </label>
      {data.saveToMemory ? (
        <>
          <label className="field checkbox-field">
            <input
              type="checkbox"
              checked={data.pinMemory}
              onChange={(e) => onUpdate({ pinMemory: e.target.checked })}
            />
            <span>Pin memory for next runs</span>
          </label>
          <label className="field">
            <span>Memory title</span>
            <input
              type="text"
              placeholder={data.label || "Output"}
              value={data.memoryTitle ?? ""}
              onChange={(e) => onUpdate({ memoryTitle: e.target.value })}
            />
          </label>
        </>
      ) : null}
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.asFinalResult}
          onChange={(e) => onUpdate({ asFinalResult: e.target.checked })}
        />
        <span>Use as final run result</span>
      </label>
      <label className="field checkbox-field">
        <input
          type="checkbox"
          checked={data.includeFilesChanged}
          onChange={(e) => onUpdate({ includeFilesChanged: e.target.checked })}
        />
        <span>Include changed files in output</span>
      </label>
    </>
  );
}

type MemorySettingsProps = {
  label: string;
  memoryIds: string[];
  memories: OutputMemory[];
  linkable: OutputMemory[];
  linkQuery: string;
  activeWorkflowId: string | null;
  onLinkQueryChange: (value: string) => void;
  onUpdate: (patch: { label?: string; memoryIds?: string[] }) => void;
  onLinkMemory: (memoryId: string) => Promise<unknown>;
};

function MemorySettings({
  label,
  memoryIds,
  memories,
  linkable,
  linkQuery,
  activeWorkflowId,
  onLinkQueryChange,
  onUpdate,
  onLinkMemory,
}: MemorySettingsProps) {
  const selectedIds = new Set(memoryIds);
  const available = [
    ...memories.filter((m) => m.origin !== "linkable"),
    ...linkable.filter((m) => !selectedIds.has(m.id)),
  ];
  const q = linkQuery.trim().toLowerCase();
  const filtered = available.filter((m) => {
    if (!q) return true;
    return (
      m.title.toLowerCase().includes(q) ||
      m.body.toLowerCase().includes(q) ||
      (m.sourceWorkflowName ?? "").toLowerCase().includes(q)
    );
  });

  const toggle = async (memory: OutputMemory) => {
    const ids = [...memoryIds];
    const index = ids.indexOf(memory.id);
    if (index >= 0) {
      ids.splice(index, 1);
      onUpdate({ memoryIds: ids });
      return;
    }
    if (
      memory.workflowId !== activeWorkflowId &&
      !memories.some((m) => m.id === memory.id)
    ) {
      await onLinkMemory(memory.id);
    }
    ids.push(memory.id);
    onUpdate({ memoryIds: ids });
  };

  return (
    <>
      <label className="field">
        <span>Label</span>
        <input
          type="text"
          value={label}
          onChange={(e) => onUpdate({ label: e.target.value })}
        />
      </label>
      <p className="hint">
        Selected memories are injected into the run context. Memories from
        other workflows are linked into this workflow’s library.
      </p>
      <label className="field">
        <span>Search</span>
        <input
          type="search"
          value={linkQuery}
          placeholder="Filter by title or workflow…"
          onChange={(e) => onLinkQueryChange(e.target.value)}
        />
      </label>
      <div className="memory-picker">
        {filtered.length === 0 ? (
          <p className="muted">No memories available to select.</p>
        ) : (
          filtered.map((memory) => {
            const checked = selectedIds.has(memory.id);
            const fromOther =
              memory.workflowId !== activeWorkflowId ||
              memory.origin === "linked" ||
              memory.origin === "linkable";
            return (
              <label key={memory.id} className="memory-picker-row">
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => void toggle(memory)}
                />
                <span className="memory-picker-copy">
                  <span className="memory-picker-title">{memory.title}</span>
                  {fromOther && memory.sourceWorkflowName ? (
                    <span className="memory-origin-badge">
                      From {memory.sourceWorkflowName}
                    </span>
                  ) : (
                    <span className="memory-picker-meta">This workflow</span>
                  )}
                </span>
              </label>
            );
          })
        )}
      </div>
    </>
  );
}

type AgentSettingsProps = {
  provider: AgentProviderId;
  model: string | null | undefined;
  skillNames: string[];
  skills: Skill[];
  providerModels: ProviderModels[];
  onUpdate: (patch: {
    model?: string | null;
    skillNames?: string[];
    skillName?: null;
  }) => void;
  onRefreshModels: () => void;
};

function AgentSettings({
  provider,
  model,
  skillNames,
  skills,
  providerModels,
  onUpdate,
  onRefreshModels,
}: AgentSettingsProps) {
  const catalog = modelsForProvider(providerModels, provider);
  const selectedModel = model || catalog.defaultModel;
  const selectedOption = catalog.models.find((m) => m.id === selectedModel);
  const savedModelIsCustom = catalog.allowCustom && !selectedOption;
  const [customModelSelected, setCustomModelSelected] = useState(false);
  const [customModelDraft, setCustomModelDraft] = useState(
    savedModelIsCustom ? selectedModel : "",
  );
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const showCustomModel = savedModelIsCustom || customModelSelected;
  const providerSkills = skills.filter((s) => s.providers.includes(provider));

  const invocation =
    skillNames.length > 0
      ? `${skillNames.map((s) => `/${s}`).join(" ")} <prompt>`
      : null;

  return (
    <>
      <div className="agent-model-controls">
        <div className="field agent-provider-field">
          <span>Provider</span>
          <strong className="agent-provider-value">{agentLabel(provider)}</strong>
        </div>

        <div className="field model-select-field">
          <span>Model</span>
          <DropdownMenu
            open={modelMenuOpen}
            onOpenChange={setModelMenuOpen}
            className="model-select"
          >
            <DropdownMenuTrigger
              className="model-select-trigger"
              aria-label="Select model"
            >
              <span className="model-select-value">
                {showCustomModel
                  ? customModelDraft.trim() || "Custom…"
                  : selectedOption?.label || selectedModel}
              </span>
              <ChevronDownIcon />
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="start"
              side="bottom"
              className="model-select-menu"
              aria-label="Models"
            >
              {catalog.models.map((option) => {
                const selected =
                  !showCustomModel && option.id === selectedModel;
                return (
                  <MenuItem
                    key={option.id}
                    className="model-select-option"
                    aria-checked={selected}
                    role="menuitemradio"
                    title={option.description || undefined}
                    onSelect={() => {
                      setCustomModelSelected(false);
                      setCustomModelDraft("");
                      setModelMenuOpen(false);
                      onUpdate({ model: option.id });
                    }}
                  >
                    <span className="model-select-option-copy">
                      <span className="model-select-option-label">
                        {option.label}
                      </span>
                      {option.description ? (
                        <span className="model-select-option-description">
                          {option.description}
                        </span>
                      ) : null}
                    </span>
                    {selected ? <CheckIcon /> : null}
                  </MenuItem>
                );
              })}
              {catalog.allowCustom ? (
                <MenuItem
                  className="model-select-option"
                  aria-checked={showCustomModel}
                  role="menuitemradio"
                  onSelect={() => {
                    setCustomModelSelected(true);
                    setCustomModelDraft(
                      savedModelIsCustom ? selectedModel : "",
                    );
                    setModelMenuOpen(false);
                  }}
                >
                  <span className="model-select-option-copy">
                    <span className="model-select-option-label">Custom…</span>
                    <span className="model-select-option-description">
                      Enter a model alias or ID
                    </span>
                  </span>
                  {showCustomModel ? <CheckIcon /> : null}
                </MenuItem>
              ) : null}
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        <button
          type="button"
          className="ghost agent-model-refresh"
          title="Refresh models from the agent / Cursor IDE"
          onClick={onRefreshModels}
        >
          Refresh
        </button>
      </div>

      <p className="hint">
        {catalog.source === "discovered"
          ? `Loaded ${catalog.models.length} models${
              provider === "cursor" ? " from Cursor" : " from agent"
          }`
          : catalog.available
            ? "Using model aliases supported by this CLI"
            : catalog.error
              ? `Using fallback — ${catalog.error}`
              : "Using fallback model list"}
      </p>

      {catalog.allowCustom && showCustomModel ? (
        <div className="custom-model-fields">
          <label className="field">
            <span>Custom model ID</span>
            <input
              type="text"
              value={customModelDraft}
              placeholder={
                provider === "opencode"
                  ? "provider/model"
                  : "model alias or id"
              }
              autoFocus
              onChange={(e) => {
                const value = e.target.value;
                setCustomModelDraft(value);
                onUpdate({ model: value.trim() ? value : null });
              }}
            />
          </label>
          <p className="hint">
            {customModelDraft.trim() ? (
              <>
                CLI will use <code>--model {customModelDraft.trim()}</code>
              </>
            ) : (
              "Enter the model ID to pass to the CLI."
            )}
          </p>
        </div>
      ) : null}

      <div className="field">
        <span>Skills</span>
        <SkillPicker
          skills={providerSkills}
          selectedNames={skillNames}
          onChange={(names) => onUpdate({ skillNames: names, skillName: null })}
        />
      </div>

      {invocation ? (
        <p className="hint">
          Run will invoke <code>{invocation}</code>
        </p>
      ) : (
        <p className="hint">No skills — freeform prompt.</p>
      )}
    </>
  );
}

function ChevronDownIcon() {
  return (
    <svg
      className="model-select-chevron"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      aria-hidden
    >
      <path
        d="m4 6 4 4 4-4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
      />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg
      className="model-select-check"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      aria-hidden
    >
      <path
        d="m3.5 8 3 3 6-6"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.6"
      />
    </svg>
  );
}
