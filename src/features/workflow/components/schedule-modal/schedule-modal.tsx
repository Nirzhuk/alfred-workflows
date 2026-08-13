import { useEffect, useMemo, useState } from "react";
import { Modal, ModalHeader } from "../../../../components/modal";
import { useWorkflowStore } from "../../store";
import { SCHEDULE_PRESETS } from "../../types";

function formatNextRun(value: string | null | undefined) {
  if (!value) return "—";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

type Props = {
  workflowId: string;
  workflowName: string;
  onClose: () => void;
};

export function ScheduleModal({ workflowId, workflowName, onClose }: Props) {
  const schedule = useWorkflowStore((s) => s.schedule);
  const loading = useWorkflowStore((s) => s.loading);
  const loadSchedule = useWorkflowStore((s) => s.loadSchedule);
  const saveSchedule = useWorkflowStore((s) => s.saveSchedule);
  const clearSchedule = useWorkflowStore((s) => s.clearSchedule);

  const [enabled, setEnabled] = useState(false);
  const [presetId, setPresetId] = useState(SCHEDULE_PRESETS[2].id);
  const [customCron, setCustomCron] = useState("");
  const [useCustom, setUseCustom] = useState(false);

  useEffect(() => {
    void loadSchedule(workflowId);
  }, [workflowId, loadSchedule]);

  useEffect(() => {
    if (!schedule || schedule.workflowId !== workflowId) {
      setEnabled(false);
      setUseCustom(false);
      setPresetId(SCHEDULE_PRESETS[2].id);
      setCustomCron("");
      return;
    }

    setEnabled(schedule.enabled);
    const matched = SCHEDULE_PRESETS.find((p) => p.cron === schedule.cron);
    if (matched) {
      setUseCustom(false);
      setPresetId(matched.id);
      setCustomCron("");
    } else {
      setUseCustom(true);
      setCustomCron(schedule.cron);
    }
  }, [schedule, workflowId]);

  const cron = useMemo(() => {
    if (useCustom) return customCron.trim();
    return (
      SCHEDULE_PRESETS.find((p) => p.id === presetId)?.cron ??
      SCHEDULE_PRESETS[2].cron
    );
  }, [useCustom, customCron, presetId]);

  const scheduleForWorkflow =
    schedule?.workflowId === workflowId ? schedule : null;

  return (
    <Modal
      size="md"
      onClose={onClose}
      labelledBy="schedule-modal-title"
    >
      <ModalHeader
        eyebrow="Schedule"
        title={workflowName}
        titleId="schedule-modal-title"
        actions={
          <button type="button" className="ghost" onClick={onClose}>
            Close
          </button>
        }
      />

      <div className="schedule-modal-body">
        <p className="muted">
          Choose when this automation should run while Agentflow is open.
        </p>

        <label className="field checkbox-field">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
          />
          <span>Enable schedule</span>
        </label>

        <label className="field">
          <span>When</span>
          <select
            value={useCustom ? "__custom__" : presetId}
            onChange={(e) => {
              if (e.target.value === "__custom__") {
                setUseCustom(true);
                return;
              }
              setUseCustom(false);
              setPresetId(e.target.value);
            }}
          >
            {SCHEDULE_PRESETS.map((preset) => (
              <option key={preset.id} value={preset.id}>
                {preset.label}
              </option>
            ))}
            <option value="__custom__">Custom cron…</option>
          </select>
        </label>

        {useCustom ? (
          <label className="field">
            <span>Cron (sec min hour day month dow)</span>
            <input
              type="text"
              value={customCron}
              placeholder="0 0 9 * * 1-5"
              onChange={(e) => setCustomCron(e.target.value)}
            />
          </label>
        ) : (
          <p className="hint">
            Cron: <code>{cron}</code>
          </p>
        )}

        <p className="hint">
          Next run:{" "}
          <strong>{formatNextRun(scheduleForWorkflow?.nextRunAt)}</strong>
        </p>

        <div className="schedule-actions">
          <button
            type="button"
            className="primary"
            disabled={loading || !cron}
            onClick={() => {
              void (async () => {
                await saveSchedule({ workflowId, cron, enabled });
                if (!useWorkflowStore.getState().error) onClose();
              })();
            }}
          >
            Save schedule
          </button>
          {scheduleForWorkflow ? (
            <button
              type="button"
              className="ghost"
              disabled={loading}
              onClick={() => {
                void (async () => {
                  await clearSchedule(workflowId);
                  if (!useWorkflowStore.getState().error) onClose();
                })();
              }}
            >
              Remove
            </button>
          ) : null}
        </div>
      </div>
    </Modal>
  );
}
