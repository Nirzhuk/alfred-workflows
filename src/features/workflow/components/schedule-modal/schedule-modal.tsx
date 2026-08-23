import { useEffect, useMemo, useState } from "react";
import { Icon } from "../../../../components/icon";
import { Modal, ModalHeader } from "../../../../components/modal";
import { SelectControl } from "../../../../components/select-control";
import {
  formatNextRunLabel,
  previewNextRunAt,
} from "../../schedule-label";
import {
  cronToPicker,
  DEFAULT_SCHEDULE_PICKER,
  parseTimeInput,
  pickerToCron,
  pickerUsesTime,
  SCHEDULE_REPEAT_OPTIONS,
  SCHEDULE_WEEKDAYS,
  timeInputValue,
  type SchedulePickerValue,
  type ScheduleRepeat,
} from "../../schedule-picker";
import { useWorkflowStore } from "../../store";

type Props = {
  workflowId: string;
  workflowName: string;
  onClose: () => void;
};

function toggleDay(days: number[], day: number): number[] {
  if (days.includes(day)) {
    const next = days.filter((value) => value !== day);
    return next.length > 0 ? next : days;
  }
  return [...days, day];
}

export function ScheduleModal({ workflowId, workflowName, onClose }: Props) {
  const schedule = useWorkflowStore((s) => s.schedule);
  const loading = useWorkflowStore((s) => s.loading);
  const loadSchedule = useWorkflowStore((s) => s.loadSchedule);
  const saveSchedule = useWorkflowStore((s) => s.saveSchedule);
  const clearSchedule = useWorkflowStore((s) => s.clearSchedule);

  const [enabled, setEnabled] = useState(false);
  const [picker, setPicker] = useState<SchedulePickerValue>(DEFAULT_SCHEDULE_PICKER);
  const [cronMode, setCronMode] = useState(false);
  const [customCron, setCustomCron] = useState("");

  useEffect(() => {
    void loadSchedule(workflowId);
  }, [workflowId, loadSchedule]);

  useEffect(() => {
    if (!schedule || schedule.workflowId !== workflowId) {
      setEnabled(false);
      setPicker(DEFAULT_SCHEDULE_PICKER);
      setCronMode(false);
      setCustomCron("");
      return;
    }

    setEnabled(schedule.enabled);
    const parsed = cronToPicker(schedule.cron);
    if (parsed) {
      setPicker(parsed);
      setCronMode(false);
      setCustomCron("");
    } else {
      setCronMode(true);
      setCustomCron(schedule.cron);
    }
  }, [schedule, workflowId]);

  const cron = useMemo(() => {
    if (cronMode) return customCron.trim();
    return pickerToCron(picker);
  }, [cronMode, customCron, picker]);

  const scheduleForWorkflow =
    schedule?.workflowId === workflowId ? schedule : null;
  const savedNextRun =
    scheduleForWorkflow?.cron === cron
      ? formatNextRunLabel(scheduleForWorkflow.nextRunAt)
      : null;
  const nextRunLabel =
    savedNextRun ?? formatNextRunLabel(previewNextRunAt(cron));
  const showTime = !cronMode && pickerUsesTime(picker.repeat);

  return (
    <Modal
      size="md"
      onClose={onClose}
      labelledBy="schedule-modal-title"
      describedBy="schedule-modal-description"
    >
      <ModalHeader
        leading={
          <span className="modal-identity-icon">
            <Icon name="clock" size={20} />
          </span>
        }
        title={`Schedule ${workflowName}`}
        titleId="schedule-modal-title"
        description="Choose when this automation should run while Alfred is open."
        descriptionId="schedule-modal-description"
        actions={
          <button
            type="button"
            className="ghost modal-close-button"
            aria-label="Close"
            onClick={onClose}
          >
            <Icon name="x" size={16} />
          </button>
        }
      />

      <div className="schedule-modal-body">
        <label className="field checkbox-field">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
          />
          <span>Enable schedule</span>
        </label>

        {cronMode ? (
          <label className="field">
            <span>Cron expression</span>
            <input
              type="text"
              value={customCron}
              placeholder="0 0 9 * * 1-5"
              spellCheck={false}
              autoCapitalize="off"
              autoCorrect="off"
              onChange={(e) => setCustomCron(e.target.value)}
            />
          </label>
        ) : (
          <>
            <label className="field">
              <span>Repeat</span>
              <SelectControl
                value={picker.repeat}
                onChange={(e) => {
                  const repeat = e.target.value as ScheduleRepeat;
                  setPicker((current) => ({
                    ...current,
                    repeat,
                    days:
                      repeat === "weekly" && current.days.length === 0
                        ? [1]
                        : current.days,
                  }));
                }}
              >
                {SCHEDULE_REPEAT_OPTIONS.map((option) => (
                  <option key={option.id} value={option.id}>
                    {option.label}
                  </option>
                ))}
              </SelectControl>
            </label>

            {picker.repeat === "weekly" ? (
              <div className="field">
                <span id="schedule-days-label">Days</span>
                <div
                  className="schedule-day-row"
                  role="group"
                  aria-labelledby="schedule-days-label"
                >
                  {SCHEDULE_WEEKDAYS.map((day) => {
                    const selected = picker.days.includes(day.value);
                    return (
                      <button
                        key={day.value}
                        type="button"
                        className={
                          selected ? "schedule-day is-selected" : "schedule-day"
                        }
                        aria-pressed={selected}
                        aria-label={day.name}
                        title={day.name}
                        onClick={() =>
                          setPicker((current) => ({
                            ...current,
                            days: toggleDay(current.days, day.value),
                          }))
                        }
                      >
                        {day.label}
                      </button>
                    );
                  })}
                </div>
              </div>
            ) : null}

            {showTime ? (
              <label className="field">
                <span>Time</span>
                <input
                  type="time"
                  className="schedule-time-input"
                  value={timeInputValue(picker.hour, picker.minute)}
                  step={60}
                  onChange={(e) => {
                    const next = parseTimeInput(e.target.value);
                    if (!next) return;
                    setPicker((current) => ({ ...current, ...next }));
                  }}
                />
              </label>
            ) : null}
          </>
        )}

        {nextRunLabel ? (
          <div className="field">
            <span>Next run</span>
            <p className="schedule-next-run">{nextRunLabel}</p>
          </div>
        ) : null}

        <label className="field checkbox-field">
          <input
            type="checkbox"
            checked={cronMode}
            onChange={(e) => {
              const next = e.target.checked;
              if (next) {
                setCustomCron(cron || "0 0 9 * * 1-5");
                setCronMode(true);
                return;
              }
              const parsed = cronToPicker(customCron);
              if (parsed) setPicker(parsed);
              setCronMode(false);
            }}
          />
          <span>Use a cron expression</span>
        </label>

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
