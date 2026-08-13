import { useEffect, useState } from "react";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useWorkflowStore } from "../../store";
import type { FileTriggerConfig, Trigger, TriggerSource } from "../../types";

function formatWhen(value: string | null | undefined) {
  if (!value) return "never";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

function fileConfig(trigger: Trigger): FileTriggerConfig {
  const config = trigger.config as FileTriggerConfig;
  return {
    path: config?.path ?? "",
    pattern: config?.pattern ?? "",
    debounceMs: config?.debounceMs ?? 2000,
  };
}

function hookUrl(baseUrl: string | null, trigger: Trigger) {
  if (!baseUrl) return null;
  return `${baseUrl}/hooks/${trigger.id}`;
}

function curlFor(baseUrl: string | null, trigger: Trigger) {
  const url = hookUrl(baseUrl, trigger);
  if (!url || !trigger.secret) return null;
  return `curl -X POST ${url} \\\n  -H "X-Alfred-Token: ${trigger.secret}" \\\n  -H "Content-Type: application/json" \\\n  -d '{"hello":"world"}'`;
}

type Props = {
  workflowId: string;
  workflowName: string;
  onClose: () => void;
};

export function TriggersModal({ workflowId, workflowName, onClose }: Props) {
  const triggers = useWorkflowStore((s) => s.triggers);
  const webhookBase = useWorkflowStore((s) => s.webhookBaseUrl);
  const loading = useWorkflowStore((s) => s.loading);
  const loadTriggers = useWorkflowStore((s) => s.loadTriggers);
  const saveTrigger = useWorkflowStore((s) => s.saveTrigger);
  const removeTrigger = useWorkflowStore((s) => s.removeTrigger);
  const testTrigger = useWorkflowStore((s) => s.testTrigger);

  const [source, setSource] = useState<TriggerSource>("file");
  const [label, setLabel] = useState("");
  const [path, setPath] = useState("");
  const [pattern, setPattern] = useState("");
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    void loadTriggers(workflowId);
  }, [workflowId, loadTriggers]);

  const canAdd = source === "webhook" || path.trim().length > 0;

  async function copy(id: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(id);
      window.setTimeout(() => setCopied(null), 1500);
    } catch {
      /* clipboard blocked — the value is on screen anyway */
    }
  }

  async function addTrigger() {
    const created = await saveTrigger({
      workflowId,
      source,
      label: label.trim(),
      config:
        source === "file"
          ? { path: path.trim(), pattern: pattern.trim(), debounceMs: 2000 }
          : {},
    });
    if (created) {
      setLabel("");
      setPath("");
      setPattern("");
    }
  }

  return (
    <Modal size="md" onClose={onClose} labelledBy="triggers-modal-title">
      <ModalHeader
        eyebrow="Triggers"
        title={workflowName}
        titleId="triggers-modal-title"
        actions={
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
        }
      />

      <div className="schedule-modal-body">
        <p className="muted">
          Start this automation when something happens — a file changes, or an
          HTTP request arrives. Triggers only fire while Alfred is open.
        </p>

          {triggers.length === 0 ? (
            <p className="hint">No triggers yet.</p>
          ) : (
            <ul className="trigger-list">
              {triggers.map((trigger) => {
                const config = fileConfig(trigger);
                const url = hookUrl(webhookBase, trigger);
                const curl = curlFor(webhookBase, trigger);

                return (
                  <li key={trigger.id} className="trigger-item">
                    <div className="trigger-item-head">
                      <strong>
                        {trigger.label ||
                          (trigger.source === "file"
                            ? "File change"
                            : "Webhook")}
                      </strong>
                      <span className="schedule-badge">{trigger.source}</span>
                      <label className="checkbox-field">
                        <input
                          type="checkbox"
                          checked={trigger.enabled}
                          onChange={(e) => {
                            void saveTrigger({
                              id: trigger.id,
                              workflowId,
                              source: trigger.source,
                              label: trigger.label,
                              config: trigger.config as Record<string, unknown>,
                              enabled: e.target.checked,
                            });
                          }}
                        />
                        <span>Enabled</span>
                      </label>
                    </div>

                    {trigger.source === "file" ? (
                      <p className="hint">
                        Watching <code>{config.path}</code>
                        {config.pattern ? (
                          <>
                            {" "}
                            matching <code>{config.pattern}</code>
                          </>
                        ) : null}
                      </p>
                    ) : url ? (
                      <>
                        <p className="hint">
                          <code>POST {url}</code>
                        </p>
                        <div className="schedule-actions">
                          <button
                            type="button"
                            className="ghost"
                            onClick={() => void copy(trigger.id, curl ?? url)}
                          >
                            {copied === trigger.id ? "Copied" : "Copy curl"}
                          </button>
                        </div>
                      </>
                    ) : (
                      <p className="hint">
                        Listener is not running — port in use? Set
                        <code> ALFRED_HTTP_PORT</code> and restart.
                      </p>
                    )}

                    <p className="hint">
                      Last fired: <strong>{formatWhen(trigger.lastFiredAt)}</strong>
                    </p>

                    <div className="schedule-actions">
                      <button
                        type="button"
                        className="ghost"
                        disabled={loading}
                        onClick={() => void testTrigger(trigger.id)}
                      >
                        Test run
                      </button>
                      <button
                        type="button"
                        className="ghost danger"
                        disabled={loading}
                        onClick={() => void removeTrigger(trigger.id)}
                      >
                        Remove
                      </button>
                    </div>
                  </li>
                );
              })}
            </ul>
          )}

          <hr />

          <label className="field">
            <span>Add trigger</span>
            <select
              value={source}
              onChange={(e) => setSource(e.target.value as TriggerSource)}
            >
              <option value="file">File change</option>
              <option value="webhook">Webhook (HTTP POST)</option>
            </select>
          </label>

          <label className="field">
            <span>Name (optional)</span>
            <input
              type="text"
              value={label}
              placeholder={source === "file" ? "Repo saves" : "Slack messages"}
              onChange={(e) => setLabel(e.target.value)}
            />
          </label>

          {source === "file" ? (
            <>
              <label className="field">
                <span>Folder or file to watch</span>
                <input
                  type="text"
                  value={path}
                  placeholder="/Users/you/code/my-project"
                  onChange={(e) => setPath(e.target.value)}
                />
              </label>
              <label className="field">
                <span>Only these files (optional)</span>
                <input
                  type="text"
                  value={pattern}
                  placeholder="*.ts,*.tsx"
                  onChange={(e) => setPattern(e.target.value)}
                />
              </label>
              <p className="hint">
                <code>.git</code>, <code>node_modules</code>, <code>target</code>{" "}
                and other build folders are ignored. Bursts of saves collapse
                into one run.
              </p>
            </>
          ) : (
            <p className="hint">
              A URL and token are generated on save. The listener is bound to
              localhost — to accept events from Slack or GitHub, point a tunnel
              at it.
            </p>
          )}

          <div className="schedule-actions">
            <button
              type="button"
              className="primary"
              disabled={loading || !canAdd}
              onClick={() => void addTrigger()}
            >
              Add trigger
            </button>
          </div>
        </div>
    </Modal>
  );
}
